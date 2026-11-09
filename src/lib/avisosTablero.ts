// src/lib/avisosTablero.ts
// Avisos efímeros del tablero (ping y recorrido): espejo de `AvisoTablero` en
// src-tauri/src/tablero_general/tablero.rs. No son estado de la partida: no se guardan en disco,
// solo se envían (hoy Rust no hace nada más que validarlos: ahí se enganchará la red) y se pintan.
//
//  * Salida: lo que haces tú. Ya lo dibujas en tu tablero; aquí solo se avisa a Rust.
//  * Entrada: lo que hacen otros. Rust lo emitirá como evento de Tauri "aviso-tablero" cuando haya red.
import { invoke } from "$lib/backend";
import { listen } from "$lib/backend";
import type { Casilla } from "$lib/partida.svelte";

export type AvisoTablero =
  | { Ping: { id_mapa: number; casilla: Casilla } }
  | { Recorrido: { id_mapa: number; casillas: Casilla[] } }
  | { RecorridoBorrado: { id_mapa: number } }
  /** Las instancias que el emisor tiene con el foco (no va ligado a un mapa) */
  | { Foco: { ids_instancias: number[] } }
  /** Alguien arrastra estas fichas y ahora pasan por esas casillas (centro de cada una); la posición real se guarda al soltar */
  | { Arrastre: { id_mapa: number; piezas: PiezaArrastrada[] } }
  /** Esas fichas ya no se arrastran */
  | { ArrastreFin: { id_mapa: number; ids_instancias: number[] } };

export interface PiezaArrastrada {
  id_instancia: number;
  casilla: Casilla;
}

export const EVENTO_AVISO = "aviso-tablero";

export const avisoPing = (id_mapa: number, casilla: Casilla): AvisoTablero => ({ Ping: { id_mapa, casilla } });
export const avisoRecorrido = (id_mapa: number, casillas: readonly Casilla[]): AvisoTablero => ({
  Recorrido: { id_mapa, casillas: [...casillas] }
});
export const avisoRecorridoBorrado = (id_mapa: number): AvisoTablero => ({ RecorridoBorrado: { id_mapa } });
export const avisoArrastre = (id_mapa: number, piezas: readonly PiezaArrastrada[]): AvisoTablero => ({
  Arrastre: { id_mapa, piezas: piezas.map((p) => ({ ...p })) }
});
export const avisoArrastreFin = (id_mapa: number, ids_instancias: readonly number[]): AvisoTablero => ({
  ArrastreFin: { id_mapa, ids_instancias: [...ids_instancias] }
});
export const avisoFoco = (ids_instancias: readonly number[]): AvisoTablero => ({ Foco: { ids_instancias: [...ids_instancias] } });

/** Avisa a Rust. Un aviso perdido no importa: nunca debe romper el tablero. */
export function enviarAviso(aviso: AvisoTablero): void {
  invoke("avisar_tablero", { aviso }).catch((err) => console.warn("Aviso del tablero no enviado:", err));
}

/** Escucha los avisos de otros jugadores. Devuelve la función que deja de escuchar (para `onMount`). */
export function alRecibirAviso(cb: (aviso: AvisoTablero) => void): () => void {
  let quitar: (() => void) | null = null;
  let terminado = false;
  try {
    listen<AvisoTablero>(EVENTO_AVISO, (e) => cb(e.payload))
      .then((fn) => (terminado ? fn() : (quitar = fn)))
      .catch((err) => console.warn("Eventos del tablero no disponibles:", err));
  } catch (err) {
    console.warn("Eventos del tablero no disponibles:", err);
  }
  return () => {
    terminado = true;
    quitar?.();
  };
}
