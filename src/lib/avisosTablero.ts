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
import type { ColorMarca } from "$lib/colorMarca";

export type AvisoTablero =
  /** `color`: el de quien lo hace (los avisos de antes no lo traen: verde) */
  | { Ping: { id_mapa: number; casilla: Casilla; color?: ColorMarca } }
  /** `jugador`: de quién es (0 = el host); lo pone el host al reenviarla, lo que mande el cliente no vale */
  | { Recorrido: { id_mapa: number; casillas: Casilla[]; color?: ColorMarca; jugador?: number } }
  | { RecorridoBorrado: { id_mapa: number; jugador?: number } }
  /** Las instancias que el emisor tiene con el foco (no va ligado a un mapa) */
  | { Foco: { ids_instancias: number[] } }
  /** Alguien arrastra estas fichas y ahora pasan por esas casillas (centro de cada una); la posición real se guarda al soltar */
  | { Arrastre: { id_mapa: number; piezas: PiezaArrastrada[]; n?: number } }
  /** Esas fichas ya no se arrastran */
  | { ArrastreFin: { id_mapa: number; ids_instancias: number[]; n?: number } };

export interface PiezaArrastrada {
  id_instancia: number;
  casilla: Casilla;
}

export const EVENTO_AVISO = "aviso-tablero";

export const avisoPing = (id_mapa: number, casilla: Casilla, color: ColorMarca): AvisoTablero => ({ Ping: { id_mapa, casilla, color: [...color] } });
export const avisoRecorrido = (id_mapa: number, casillas: readonly Casilla[], color: ColorMarca): AvisoTablero => ({
  Recorrido: { id_mapa, casillas: [...casillas], color: [...color] }
});
export const avisoRecorridoBorrado = (id_mapa: number): AvisoTablero => ({ RecorridoBorrado: { id_mapa } });
/**
 * Número de orden de los avisos de arrastre de ESTA pantalla. Por la conexión directa el arrastre va por un canal sin
 * reintentos ni orden (protocolo 7): quien lo recibe ignora uno más viejo que el último que vio (ver
 * `arrastreAjeno`). `ArrastreFin` lleva el siguiente número, mayor que el de cualquier `Arrastre` anterior.
 */
let numeroArrastre = 0;
export const avisoArrastre = (id_mapa: number, piezas: readonly PiezaArrastrada[]): AvisoTablero => ({
  Arrastre: { id_mapa, piezas: piezas.map((p) => ({ ...p })), n: ++numeroArrastre }
});
export const avisoArrastreFin = (id_mapa: number, ids_instancias: readonly number[]): AvisoTablero => ({
  ArrastreFin: { id_mapa, ids_instancias: [...ids_instancias], n: ++numeroArrastre }
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
