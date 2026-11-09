// src/lib/arrastreFicha.svelte.ts
//
// Arrastrar fichas entre el banco, el tablero y «Fichas activas». Va con eventos de puntero y no con
// el drag & drop HTML5: en Windows, Tauri se queda esos eventos para su drop de archivos.
//
// Cada panel que admite fichas lleva `data-destino="tablero" | "activas" | "banco"`; al soltar se
// mira qué hay bajo el cursor. El tablero, además, registra cómo pasar el cursor a una casilla, y
// las tarjetas de «Fichas activas» llevan `data-indice` para reordenarlas.
// Qué hacer en cada caso lo decide `accionAlSoltar` (pura, con tests); las reglas finas (ficha
// única, casilla encajada...) las aplica Rust.
import { partida, reordenarConOcultas, type Destino, type Entidad, type Instancia } from "$lib/partida.svelte";
import { ladosInstancia, tamanoCuerpo, type Lados } from "$lib/tablero";
import { esJugador } from "$lib/modo";

/**
 * Lo que se arrastra: una ficha del banco, varias (selección con Ctrl) o una instancia (y de dónde se
 * cogió). En el tablero, `grupo` son todas las seleccionadas de ese mapa que se arrastran con ella
 * (incluida ella): se mueven juntas conservando sus distancias.
 */
export type OrigenArrastre =
  | { entidad: Entidad }
  | { entidades: Entidad[] }
  | { instancia: Instancia; desde: "activas" | "tablero"; grupo?: Instancia[] };
export type ZonaDestino = "tablero" | "activas" | "banco";

export type AccionArrastre =
  | { tipo: "instanciar"; idEntidad: number; destino: Destino | null }
  | { tipo: "instanciarVarias"; idsEntidades: number[]; destino: Destino | null }
  | { tipo: "mover"; idInstancia: number; destino: Destino }
  | { tipo: "moverVarias"; ids: number[]; idMapa: number; dx: number; dy: number }
  | { tipo: "quitarDelTablero"; idInstancia: number }
  | { tipo: "quitarVariasDelTablero"; ids: number[] }
  | { tipo: "borrar"; idInstancia: number }
  | { tipo: "borrarVarias"; ids: number[] }
  | { tipo: "reordenar"; ids: number[] };

/** Las instancias que se arrastran juntas (solo una si no hay grupo). */
export function instanciasArrastradas(origen: OrigenArrastre): Instancia[] {
  if (!("instancia" in origen)) return [];
  return origen.grupo && origen.grupo.length > 1 ? origen.grupo : [origen.instancia];
}

/** Ids de «Fichas activas» con `id` movido a la posición `indice`. */
export function moverEnLista(ids: readonly number[], id: number, indice: number): number[] {
  const resto = ids.filter((x) => x !== id);
  const i = Math.max(0, Math.min(indice, resto.length));
  return [...resto.slice(0, i), id, ...resto.slice(i)];
}

/**
 * Qué pasa al soltar `origen` sobre `zona`. `casilla` es el destino en el tablero (null si el
 * cursor no está sobre el mapa abierto); `orden` (ids de «Fichas activas») e `indice` (posición
 * bajo el cursor en esa lista), para reordenar.
 *
 * | origen \ zona          | tablero              | activas                | banco        |
 * | ficha del banco        | instanciar ahí       | instanciar sin casilla | nada         |
 * | varias del banco       | en cuadrícula        | todas sin casilla      | nada         |
 * | instancia del tablero  | mover ahí            | quitar del tablero     | borrar       |
 * | grupo del tablero      | moverlas juntas      | quitarlas del tablero  | borrarlas    |
 * | instancia de activas   | mover ahí            | reordenar              | borrar       |
 *
 * Con un grupo, el desplazamiento es el de la ficha cogida: de su casilla a `casilla`.
 */
export function accionAlSoltar(
  origen: OrigenArrastre,
  zona: ZonaDestino | null,
  casilla: Destino | null,
  orden: readonly number[] = [],
  indice: number | null = null
): AccionArrastre | null {
  if ("entidad" in origen) {
    const idEntidad = origen.entidad.id;
    if (zona === "tablero") return casilla ? { tipo: "instanciar", idEntidad, destino: casilla } : null;
    if (zona === "activas") return { tipo: "instanciar", idEntidad, destino: null };
    return null;
  }
  if ("entidades" in origen) {
    const idsEntidades = origen.entidades.map((e) => e.id);
    if (zona === "tablero") return casilla ? { tipo: "instanciarVarias", idsEntidades, destino: casilla } : null;
    if (zona === "activas") return { tipo: "instanciarVarias", idsEntidades, destino: null };
    return null;
  }
  const { id_instancia: idInstancia, coordenadas } = origen.instancia;
  const grupo = instanciasArrastradas(origen);
  if (grupo.length > 1) {
    const ids = grupo.map((i) => i.id_instancia);
    if (zona === "banco") return { tipo: "borrarVarias", ids };
    if (zona === "activas") return { tipo: "quitarVariasDelTablero", ids };
    if (zona !== "tablero" || !casilla || typeof coordenadas !== "object") return null;
    const dx = casilla.casilla.x - coordenadas.Casilla.x;
    const dy = casilla.casilla.y - coordenadas.Casilla.y;
    return { tipo: "moverVarias", ids, idMapa: casilla.id_mapa, dx, dy };
  }
  if (zona === "tablero") return casilla ? { tipo: "mover", idInstancia, destino: casilla } : null;
  if (zona === "banco") return { tipo: "borrar", idInstancia };
  if (zona !== "activas") return null;

  if (origen.desde === "activas") {
    if (indice === null) return null;
    const ids = moverEnLista(orden, idInstancia, indice);
    return ids.every((id, i) => id === orden[i]) ? null : { tipo: "reordenar", ids };
  }
  return coordenadas === "NoPuesto" ? null : { tipo: "quitarDelTablero", idInstancia };
}

/** La ficha que se ve al arrastrar (con varias, la primera). */
export function baseDe(origen: OrigenArrastre): Entidad {
  if ("entidad" in origen) return origen.entidad;
  if ("entidades" in origen) return origen.entidades[0];
  return origen.instancia.base;
}

/** Lados en el tablero de una ficha del banco: los de su cuerpo o, si es única y ya está en el tablero,
 *  los de su instancia (que se reutiliza con su giro), igual que hace Rust. */
export function ladosDeEntidad(entidad: Entidad): Lados {
  const existente = entidad.es_unico ? partida.instancias.find((i) => i.base.id === entidad.id) : undefined;
  return existente ? ladosInstancia(existente) : tamanoCuerpo(entidad.cuerpo);
}

/** Lados de lo que se arrastra (con varias, los de la primera: es la que cae bajo el cursor). */
export function ladosDe(origen: OrigenArrastre): Lados {
  if ("instancia" in origen) return ladosInstancia(origen.instancia);
  return ladosDeEntidad(baseDe(origen));
}

/** Lo que hay que hacer en el store para una acción. */
export function ejecutarAccion(accion: AccionArrastre) {
  switch (accion.tipo) {
    case "instanciar":
      return partida.instanciar(accion.idEntidad, accion.destino);
    case "instanciarVarias":
      return partida.instanciarVarias(accion.idsEntidades, accion.destino);
    case "mover":
      return partida.moverInstancia(accion.idInstancia, accion.destino);
    case "moverVarias":
      return partida.moverInstancias(accion.ids, accion.idMapa, accion.dx, accion.dy);
    case "quitarDelTablero":
      return partida.quitarDelTablero(accion.idInstancia);
    case "quitarVariasDelTablero":
      return partida.quitarVariasDelTablero(accion.ids);
    case "borrar":
      return partida.borrarInstancia(accion.idInstancia);
    case "borrarVarias":
      return partida.borrarInstancias(accion.ids);
    case "reordenar":
      return partida.reordenarInstancias(accion.ids);
  }
}

/** Píxeles que hay que mover el ratón para que una pulsación sea un arrastre y no un clic. */
export const UMBRAL_ARRASTRE_PX = 5;

export interface ArrastreEnCurso {
  origen: OrigenArrastre;
  foto: string | null;
  lados: Lados;
  /** Posición del puntero (px de la ventana) */
  x: number;
  y: number;
  zona: ZonaDestino | null;
  /** Posición bajo el cursor en «Fichas activas» (para reordenar) */
  indice: number | null;
  /** Punto por el que se cogió la ficha, en fracción de su ancho y alto (0..1). Sin él, el centro. */
  agarre: { x: number; y: number } | null;
}

type ResolverCasilla = (clientX: number, clientY: number, lados: Lados, agarre: ArrastreEnCurso["agarre"]) => Destino | null;

class ArrastreFicha {
  /** El arrastre en marcha (null si no hay). El tablero y «Fichas activas» lo leen para dibujarlo. */
  actual = $state<ArrastreEnCurso | null>(null);

  #resolverCasilla: ResolverCasilla | null = null;
  #ignorarClic = false;
  #quitar: (() => void) | null = null;

  /** El tablero dice cómo pasar el cursor a una casilla del mapa abierto. Devuelve cómo dejar de hacerlo. */
  registrarTablero(resolver: ResolverCasilla) {
    this.#resolverCasilla = resolver;
    return () => {
      if (this.#resolverCasilla === resolver) this.#resolverCasilla = null;
    };
  }

  /**
   * Llamar en el `pointerdown` de algo que se puede arrastrar. Si el ratón se mueve más del umbral
   * empieza el arrastre; si se suelta antes, es un clic y se llama a `alClic` (si se da).
   */
  preparar(e: PointerEvent, origen: OrigenArrastre, alClic?: () => void, agarre: ArrastreEnCurso["agarre"] = null) {
    if (e.button !== 0 || this.actual) return;
    if (esJugador) {
      // Fase 1: el jugador aún no mueve fichas (docs/red.md). Un clic sigue siendo un clic (foco).
      const alSoltar = () => {
        window.removeEventListener("pointerup", alSoltar);
        alClic?.();
      };
      window.addEventListener("pointerup", alSoltar);
      return;
    }
    this.#terminar();
    const inicio = { x: e.clientX, y: e.clientY };
    const base = baseDe(origen);
    const lados = ladosDe(origen);
    let empezado = false;

    const mover = (ev: PointerEvent) => {
      if (!empezado) {
        if (Math.hypot(ev.clientX - inicio.x, ev.clientY - inicio.y) <= UMBRAL_ARRASTRE_PX) return;
        empezado = true;
      }
      this.#actualizar(origen, base.foto_url, lados, agarre, ev.clientX, ev.clientY);
    };
    const soltar = (_ev: PointerEvent) => {
      this.#terminar();
      if (!empezado) return alClic?.();
      this.#completar(origen);
    };
    const cancelar = (ev: Event) => {
      if (ev instanceof KeyboardEvent && ev.key !== "Escape") return;
      this.#terminar();
      this.actual = null;
    };

    window.addEventListener("pointermove", mover);
    window.addEventListener("pointerup", soltar);
    window.addEventListener("pointercancel", cancelar);
    window.addEventListener("keydown", cancelar);
    this.#quitar = () => {
      window.removeEventListener("pointermove", mover);
      window.removeEventListener("pointerup", soltar);
      window.removeEventListener("pointercancel", cancelar);
      window.removeEventListener("keydown", cancelar);
    };
  }

  // Pone el arrastre en (x, y) (px de la ventana): qué zona hay debajo y, en «Fichas activas», qué posición
  #actualizar(origen: OrigenArrastre, foto: string | null, lados: Lados, agarre: ArrastreEnCurso["agarre"], x: number, y: number) {
    const zona = zonaEn(x, y);
    this.actual = { origen, foto, lados, x, y, zona, indice: zona === "activas" ? indiceEn(x, y) : null, agarre };
  }

  // Termina el arrastre en curso: hace lo que toque donde se soltó
  #completar(origen: OrigenArrastre) {
    const a = this.actual;
    this.actual = null;
    if (!a) return;
    // El clic que el navegador manda al soltar sobre el elemento de origen no debe abrir nada
    this.#ignorarClic = true;
    setTimeout(() => (this.#ignorarClic = false), 0);

    const casilla = a.zona === "tablero" ? (this.#resolverCasilla?.(a.x, a.y, a.lados, a.agarre) ?? null) : null;
    // `indice` es la posición en la lista que se VE (con el filtro de mapa puede faltar alguna): se reordena
    // ahí y luego se pasa al orden completo, que es el que guarda Rust
    const orden = partida.instanciasVisibles.map((i) => i.id_instancia);
    const accion = accionAlSoltar(origen, a.zona, casilla, orden, a.indice);
    if (accion?.tipo === "reordenar") {
      accion.ids = reordenarConOcultas(partida.instanciasOrdenadas.map((i) => i.id_instancia), accion.ids);
    }
    if (accion) void ejecutarAccion(accion);
  }

  /** Para el `onclick` de lo que se arrastra: true si ese clic es el final de un arrastre y hay que ignorarlo. */
  esClicDeArrastre() {
    return this.#ignorarClic;
  }

  /** Casilla bajo el cursor durante el arrastre (para el resaltado del tablero). */
  casillaActual(): Destino | null {
    const a = this.actual;
    if (!a || a.zona !== "tablero") return null;
    return this.#resolverCasilla?.(a.x, a.y, a.lados, a.agarre) ?? null;
  }

  /** ¿Se está arrastrando esta instancia, sola o en su grupo? (el tablero no la dibuja en su sitio mientras tanto) */
  arrastrando(idInstancia: number) {
    const o = this.actual?.origen;
    return !!o && instanciasArrastradas(o).some((i) => i.id_instancia === idInstancia);
  }

  #terminar() {
    this.#quitar?.();
    this.#quitar = null;
  }
}

/** Zona que admite fichas bajo un punto de la ventana. */
function zonaEn(x: number, y: number): ZonaDestino | null {
  const el = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-destino]");
  const zona = el?.dataset.destino;
  return zona === "tablero" || zona === "activas" || zona === "banco" ? zona : null;
}

/** Posición en «Fichas activas» (en la lista que se ve) bajo el cursor: la de la tarjeta que hay debajo, o el final. */
function indiceEn(x: number, y: number): number {
  const tarjeta = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-indice]");
  if (tarjeta?.dataset.indice !== undefined) return Number(tarjeta.dataset.indice);
  return partida.instanciasVisibles.length;
}

export const arrastre = new ArrastreFicha();
