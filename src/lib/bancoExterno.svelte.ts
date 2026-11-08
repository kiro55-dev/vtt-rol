// src/lib/bancoExterno.svelte.ts
//
// BANCO EXTERNO: apaño para sacar el banco de fichas a otra ventana del sistema, que no sale si se
// comparte solo la ventana principal (Discord). Todo lo de esta función está marcado con
// "BANCO EXTERNO" para poder quitarlo con un buscador:
//  - este archivo y src/routes/banco/
//  - los puntos marcados en BancoEntidades, PanelIzquierdo, arrastreFicha y +page.svelte
//  - los permisos de ventana en src-tauri/capabilities/default.json
//
// La ventana del banco es otro contexto de JavaScript: lee sus datos de Rust con `partida.iniciar()` y
// se avisa con eventos de Tauri. Los eventos de puntero no pasan de una ventana a otra, así que un
// arrastre que empieza en el banco va así:
//   banco: pointermove -> cursorPosition() (px de pantalla) -> evento "banco-arrastre"
//   principal: pasa esa posición a px de SU ventana y conduce el mismo `arrastre` de siempre
// (con el botón pulsado la ventana del banco sigue recibiendo los movimientos aunque el cursor salga).
import { emit, listen } from "@tauri-apps/api/event";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { cursorPosition, getCurrentWindow } from "@tauri-apps/api/window";
import { partida, type Entidad } from "$lib/partida.svelte";
import { arrastre, UMBRAL_ARRASTRE_PX } from "$lib/arrastreFicha.svelte";

const ETIQUETA = "banco";
export const EVENTO_ARRASTRE = "banco-arrastre";
export const EVENTO_ABRIR_FICHA = "banco-abrir-ficha";
export const EVENTO_PEDIR_ESTADO = "banco-pedir-estado";
export const EVENTO_CAMBIADO = "banco-cambiado";

export type EventoArrastreBanco =
  | { fase: "inicio"; idEntidad: number }
  | { fase: "mover" | "fin"; x: number; y: number }
  | { fase: "cancelar" };

/** Posición (px físicos de pantalla) y escala de la parte útil de una ventana. */
export interface VentanaEnPantalla {
  x: number;
  y: number;
  escala: number;
}

/** Posición global del cursor (px físicos) -> px de la ventana (los del navegador, con `elementFromPoint`). */
export function aVentana(cursor: { x: number; y: number }, ventana: VentanaEnPantalla) {
  return { x: (cursor.x - ventana.x) / ventana.escala, y: (cursor.y - ventana.y) / ventana.escala };
}

class BancoExterno {
  /** La ventana del banco está abierta (y el banco de la principal, oculto). */
  abierto = $state(false);

  #ventana: Promise<VentanaEnPantalla> | null = null;

  // ------------------------------------------------------------------
  // Ventana principal
  // ------------------------------------------------------------------

  async abrir() {
    if (this.abierto) return;
    const existente = await WebviewWindow.getByLabel(ETIQUETA);
    if (existente) {
      this.#vigilar(existente);
      return void (await existente.setFocus());
    }
    const ventana = new WebviewWindow(ETIQUETA, { url: "/banco", title: "Banco de fichas", width: 320, height: 640 });
    this.#vigilar(ventana);
    void ventana.once("tauri://error", (e) => {
      console.error("No se pudo abrir la ventana del banco:", e.payload);
      this.abierto = false;
    });
  }

  async cerrar() {
    this.abierto = false;
    await (await WebviewWindow.getByLabel(ETIQUETA))?.close();
  }

  #vigilar(ventana: WebviewWindow) {
    this.abierto = true;
    void ventana.once("tauri://destroyed", () => (this.abierto = false));
  }

  /**
   * Para la ventana principal, al montar. Atiende lo que viene de la del banco, recupera una ventana
   * del banco que ya estuviera abierta (recarga en desarrollo) y la cierra con la principal.
   */
  escuchar(): () => void {
    const quitar: Array<() => void> = [];
    let terminado = false;
    const guardar = (p: Promise<() => void>) =>
      p.then((fn) => (terminado ? fn() : quitar.push(fn))).catch((err) => console.warn("Banco externo no disponible:", err));

    guardar(
      listen<EventoArrastreBanco>(EVENTO_ARRASTRE, (e) => void this.#alArrastrar(e.payload))
    );
    guardar(
      listen<{ id: number | null }>(EVENTO_ABRIR_FICHA, (e) => {
        const entidad = e.payload.id === null ? null : (partida.banco.find((b) => b.id === e.payload.id) ?? null);
        void partida.abrirEditorEntidad(entidad);
        void getCurrentWindow().setFocus();
      })
    );
    guardar(listen(EVENTO_PEDIR_ESTADO, () => void this.avisarCambio(true)));
    guardar(
      getCurrentWindow().onCloseRequested(async () => {
        await this.cerrar();
      })
    );
    void WebviewWindow.getByLabel(ETIQUETA)
      .then((v) => v && this.#vigilar(v))
      .catch(() => {});

    return () => {
      terminado = true;
      quitar.forEach((fn) => fn());
    };
  }

  /** Dice a la ventana del banco que algo cambió (fichas, instancias o zona actual). */
  async avisarCambio(forzar = false) {
    if (!this.abierto && !forzar) return;
    await emit(EVENTO_CAMBIADO, { zonaId: partida.zonaId }).catch(() => {});
  }

  async #alArrastrar(ev: EventoArrastreBanco) {
    if (ev.fase === "inicio") {
      const entidad = partida.banco.find((b) => b.id === ev.idEntidad);
      if (!entidad) return;
      // La posición de la ventana se pide UNA vez; los movimientos esperan a esa misma promesa
      const w = getCurrentWindow();
      this.#ventana = Promise.all([w.innerPosition(), w.scaleFactor()]).then(([p, escala]) => ({ x: p.x, y: p.y, escala }));
      await this.#ventana;
      arrastre.externoInicio(entidad);
    } else if (ev.fase === "cancelar") {
      this.#ventana = null;
      arrastre.externoCancelar();
    } else if (this.#ventana) {
      const v = await this.#ventana;
      const p = aVentana(ev, v);
      if (ev.fase === "mover") arrastre.externoMover(p.x, p.y);
      else {
        this.#ventana = null;
        arrastre.externoFin(p.x, p.y);
      }
    }
  }

  // ------------------------------------------------------------------
  // Ventana del banco
  // ------------------------------------------------------------------

  /** Abre el editor de la ficha en la ventana principal (`null` = ficha nueva). */
  pedirAbrirFicha(id: number | null) {
    void emit(EVENTO_ABRIR_FICHA, { id });
  }

  /**
   * Como `arrastre.preparar`, pero el destino está en otra ventana: manda a la principal la posición
   * del cursor. Si se suelta casi sin moverse es un clic y se llama a `alClic`.
   */
  prepararArrastre(e: PointerEvent, entidad: Entidad, alClic: () => void) {
    if (e.button !== 0) return;
    const inicio = { x: e.clientX, y: e.clientY };
    let empezado = false;
    let enVuelo = false;
    // Con el puntero capturado los movimientos siguen llegando aunque el cursor salga de la ventana
    try {
      (e.currentTarget as Element | null)?.setPointerCapture(e.pointerId);
    } catch {
      /* sin captura sigue funcionando mientras el cursor esté dentro */
    }

    const mover = async (ev: PointerEvent) => {
      if (!empezado) {
        if (Math.hypot(ev.clientX - inicio.x, ev.clientY - inicio.y) <= UMBRAL_ARRASTRE_PX) return;
        empezado = true;
        void emit(EVENTO_ARRASTRE, { fase: "inicio", idEntidad: entidad.id } satisfies EventoArrastreBanco);
      }
      if (enVuelo) return; // una posición a la vez
      enVuelo = true;
      try {
        const p = await cursorPosition();
        await emit(EVENTO_ARRASTRE, { fase: "mover", x: p.x, y: p.y } satisfies EventoArrastreBanco);
      } finally {
        enVuelo = false;
      }
    };
    const soltar = async () => {
      quitar();
      if (!empezado) return alClic();
      const p = await cursorPosition();
      await emit(EVENTO_ARRASTRE, { fase: "fin", x: p.x, y: p.y } satisfies EventoArrastreBanco);
    };
    const cancelar = (ev: Event) => {
      if (ev instanceof KeyboardEvent && ev.key !== "Escape") return;
      quitar();
      if (empezado) void emit(EVENTO_ARRASTRE, { fase: "cancelar" } satisfies EventoArrastreBanco);
    };
    const quitar = () => {
      window.removeEventListener("pointermove", mover);
      window.removeEventListener("pointerup", soltar);
      window.removeEventListener("pointercancel", cancelar);
      window.removeEventListener("keydown", cancelar);
    };
    window.addEventListener("pointermove", mover);
    window.addEventListener("pointerup", soltar);
    window.addEventListener("pointercancel", cancelar);
    window.addEventListener("keydown", cancelar);
  }
}

export const bancoExterno = new BancoExterno();
