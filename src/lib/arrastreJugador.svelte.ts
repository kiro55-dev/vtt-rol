// src/lib/arrastreJugador.svelte.ts
//
// Arrastrar a un jugador desde la ventana «Configuración de sala» y soltarlo sobre una ficha: pasa a ser
// uno de sus propietarios (la controla y la ve entera). Vale una fila del banco, una tarjeta de «Fichas
// activas» (las dos llevan `data-id-entidad`) o una ficha del tablero (el tablero registra cómo saber qué
// ficha hay bajo el cursor). Con eventos de puntero, como arrastreFicha (el drag & drop HTML5 lo intercepta
// Tauri en Windows).
import { sala, type JugadorSala } from "$lib/sala.svelte";

const UMBRAL_PX = 5;

export interface JugadorArrastrado {
  jugador: JugadorSala;
  x: number;
  y: number;
  /** La ficha bajo el cursor (la que lo recibiría al soltar), si hay. */
  idEntidad: number | null;
}

type ResolverTablero = (clientX: number, clientY: number) => number | null;

/** La ficha (id del banco) marcada con `data-id-entidad` en un elemento o en uno de sus padres. */
export function idEntidadDe(elemento: Element | null): number | null {
  const valor = elemento?.closest<HTMLElement>("[data-id-entidad]")?.dataset.idEntidad;
  const n = valor === undefined ? NaN : Number(valor);
  return Number.isInteger(n) ? n : null;
}

class ArrastreJugador {
  actual = $state<JugadorArrastrado | null>(null);
  #tablero: ResolverTablero | null = null;

  /** El tablero dice qué ficha hay bajo un punto de la ventana. Devuelve cómo dejar de hacerlo. */
  registrarTablero(resolver: ResolverTablero) {
    this.#tablero = resolver;
    return () => {
      if (this.#tablero === resolver) this.#tablero = null;
    };
  }

  /** La ficha bajo el punto: lo marcado con `data-id-entidad` o, si no, la del tablero. */
  entidadEn(x: number, y: number): number | null {
    const el = typeof document === "undefined" ? null : document.elementFromPoint(x, y);
    return idEntidadDe(el) ?? (el?.closest("[data-destino='tablero']") ? (this.#tablero?.(x, y) ?? null) : null);
  }

  /** En el `pointerdown` de la fila de un jugador. */
  preparar(e: PointerEvent, jugador: JugadorSala) {
    if (e.button !== 0 || this.actual) return;
    e.preventDefault();
    const inicio = { x: e.clientX, y: e.clientY };
    let empezado = false;

    const mover = (ev: PointerEvent) => {
      if (!empezado && Math.hypot(ev.clientX - inicio.x, ev.clientY - inicio.y) <= UMBRAL_PX) return;
      empezado = true;
      this.actual = { jugador, x: ev.clientX, y: ev.clientY, idEntidad: this.entidadEn(ev.clientX, ev.clientY) };
    };
    const terminar = () => {
      window.removeEventListener("pointermove", mover);
      window.removeEventListener("pointerup", soltar);
      window.removeEventListener("pointercancel", cancelar);
      window.removeEventListener("keydown", cancelar);
    };
    const soltar = (ev: PointerEvent) => {
      terminar();
      const a = this.actual;
      this.actual = null;
      if (!empezado || !a) return;
      const id = this.entidadEn(ev.clientX, ev.clientY);
      if (id !== null) sala.asignar(jugador.id, id).catch((err) => console.error("asignar_propietario:", err));
    };
    const cancelar = (ev: Event) => {
      if (ev instanceof KeyboardEvent && ev.key !== "Escape") return;
      terminar();
      this.actual = null;
    };
    window.addEventListener("pointermove", mover);
    window.addEventListener("pointerup", soltar);
    window.addEventListener("pointercancel", cancelar);
    window.addEventListener("keydown", cancelar);
  }
}

export const arrastreJugador = new ArrastreJugador();
