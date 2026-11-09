// src/lib/arrastreTirada.svelte.ts
//
// Arrastrar una tirada del chat sobre el valor de una ficha: con el botón izquierdo le QUITA el
// total (daño), con el derecho se lo SUMA (curación). Los destinos son los elementos con
// `data-valor="idInstancia:idDefinicion"`: el resumen «actual/máx» de una tarjeta de «Fichas
// activas» y la fila de ese parámetro en la caja de parámetros.
//
// El cambio pasa por `partida.cambiarValorActual`, así que se ve al momento, se queda entre 0 y el
// máximo y se manda a Rust agrupado (un solo delta, igual que la rueda del ratón).
import { partida } from "$lib/partida.svelte";

/** Píxeles que hay que mover el ratón para que una pulsación sea un arrastre. */
export const UMBRAL_ARRASTRE_TIRADA_PX = 5;

/** Cuánto cambia el valor: el botón izquierdo (0) resta el total, el derecho (2) lo suma. */
export function deltaDeArrastre(boton: number, total: number): number {
  return boton === 2 ? total : -total;
}

/** Lo mismo, pero solo si esta pantalla puede cambiar ese valor (el jugador, solo en sus fichas). */
function destinoPermitido(el: Element | null | undefined) {
  const d = destinoDeValor(el);
  const i = d && partida.instancias.find((x) => x.id_instancia === d.idInstancia);
  return d && i && partida.controla(i) ? d : null;
}

/** Lee "idInstancia:idDefinicion" del elemento `data-valor` más cercano. */
export function destinoDeValor(el: Element | null | undefined): { idInstancia: number; idDefinicion: number } | null {
  const texto = el?.closest<HTMLElement>("[data-valor]")?.dataset.valor;
  const [a, b] = (texto ?? "").split(":").map(Number);
  return Number.isInteger(a) && Number.isInteger(b) ? { idInstancia: a, idDefinicion: b } : null;
}

/**
 * Al soltar el botón derecho el navegador manda el evento "contextmenu" justo después, sea cual sea
 * el elemento bajo el cursor (y algunos, como las tarjetas de «Fichas activas», abren su propio menú).
 * Tras un arrastre con el derecho se traga ese único evento, en captura para llegar antes que nadie.
 */
function silenciarMenuContextual() {
  const tragar = (ev: Event) => {
    ev.preventDefault();
    ev.stopPropagation();
    quitar();
  };
  const quitar = () => {
    window.removeEventListener("contextmenu", tragar, true);
    clearTimeout(limite);
  };
  // Si por lo que sea el evento no llega, no se queda escuchando para siempre
  const limite = setTimeout(quitar, 300);
  window.addEventListener("contextmenu", tragar, true);
}

export interface TiradaArrastrada {
  delta: number;
  x: number;
  y: number;
  /** El valor sobre el que está ahora (para resaltarlo), si hay uno. */
  destino: { idInstancia: number; idDefinicion: number } | null;
}

class ArrastreTirada {
  /** El arrastre en marcha (null si no hay). */
  actual = $state<TiradaArrastrada | null>(null);

  #quitar: (() => void) | null = null;
  #ignorarClic = false;

  /**
   * Llamar en el `pointerdown` de una línea de tirada con total válido. Botón izquierdo o derecho.
   * Si el ratón se mueve más del umbral empieza el arrastre; si no, es un clic normal.
   */
  preparar(e: PointerEvent, total: number) {
    if ((e.button !== 0 && e.button !== 2) || this.actual) return;
    // Arrastrar no debe ir seleccionando el texto de por medio (ni dejar seleccionado lo anterior)
    e.preventDefault();
    window.getSelection()?.removeAllRanges();
    this.#terminar();
    const delta = deltaDeArrastre(e.button, total);
    const inicio = { x: e.clientX, y: e.clientY };
    let empezado = false;

    const mover = (ev: PointerEvent) => {
      if (!empezado) {
        if (Math.hypot(ev.clientX - inicio.x, ev.clientY - inicio.y) <= UMBRAL_ARRASTRE_TIRADA_PX) return;
        empezado = true;
      }
      const destino = destinoPermitido(document.elementFromPoint(ev.clientX, ev.clientY));
      this.actual = { delta, x: ev.clientX, y: ev.clientY, destino };
    };
    const soltar = (ev: PointerEvent) => {
      this.#terminar();
      if (!empezado) return;
      this.actual = null;
      this.#ignorarClic = true;
      setTimeout(() => (this.#ignorarClic = false), 0);
      if (ev.button === 2) silenciarMenuContextual();
      const destino = destinoPermitido(document.elementFromPoint(ev.clientX, ev.clientY));
      if (destino) partida.cambiarValorActual(destino.idInstancia, destino.idDefinicion, delta);
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

  esClicDeArrastre() {
    return this.#ignorarClic;
  }

  /** ¿Es este el valor sobre el que está el cursor? (para resaltarlo) */
  esDestino(idInstancia: number, idDefinicion: number) {
    const d = this.actual?.destino;
    return !!d && d.idInstancia === idInstancia && d.idDefinicion === idDefinicion;
  }

  #terminar() {
    this.#quitar?.();
    this.#quitar = null;
  }
}

export const arrastreTirada = new ArrastreTirada();
