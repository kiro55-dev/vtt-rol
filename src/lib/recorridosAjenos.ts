// src/lib/recorridosAjenos.ts
//
// Las flechas (recorridos) que dibujan los DEMÁS, una por persona: así se ven todas a la vez, cada una de su color, aunque
// varias personas midan al mismo tiempo. Las flechas llegan como avisos efímeros (`Recorrido` / `RecorridoBorrado`) con
// el `jugador` que pone el host (0 = el host) y el color de quien las dibuja. Solo cuenta lo del mapa abierto.
import type { Casilla } from "$lib/partida.svelte";
import type { AvisoTablero } from "$lib/avisosTablero";
import { COLOR_VERDE, esColor, type ColorMarca } from "$lib/colorMarca";

export interface FlechaAjena {
  jugador: number;
  casillas: Casilla[];
  color: ColorMarca;
}

export class RecorridosAjenos {
  #flechas = new Map<number, FlechaAjena>();

  /** Atiende un aviso. Devuelve si cambió algo (para repintar). `idMapa`: el mapa que se está viendo. */
  recibir(aviso: AvisoTablero, idMapa: number | null): boolean {
    if ("Recorrido" in aviso) {
      const r = aviso.Recorrido;
      if (r.id_mapa !== idMapa) return false;
      const jugador = r.jugador ?? 0;
      this.#flechas.set(jugador, { jugador, casillas: r.casillas, color: esColor(r.color) ? r.color : COLOR_VERDE });
      return true;
    }
    if ("RecorridoBorrado" in aviso) {
      const r = aviso.RecorridoBorrado;
      if (r.id_mapa !== idMapa) return false;
      return this.#flechas.delete(r.jugador ?? 0);
    }
    return false;
  }

  /** Las flechas de ahora, por orden de jugador (el host primero). */
  lista(): FlechaAjena[] {
    return [...this.#flechas.values()].sort((a, b) => a.jugador - b.jugador);
  }

  /** Quita las de una persona (p. ej. si se va). */
  quitar(jugador: number): void {
    this.#flechas.delete(jugador);
  }

  /** Quita todas (al cambiar de mapa). */
  limpiar(): void {
    this.#flechas.clear();
  }
}
