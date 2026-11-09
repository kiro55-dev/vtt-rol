// src/lib/backend.ts
//
// La única puerta de la app hacia sus datos: `invoke` (comandos) y `listen` (eventos). Nada fuera de este
// archivo debe importar @tauri-apps/api/core ni /event directamente.
//  * Host: es Tauri tal cual (Rust).
//  * Jugador: contesta la conexión con la sala (src/lib/red/conexion.svelte.ts) con la réplica de lo que el
//    host le deja ver, y los eventos son locales (la conexión los emite al llegarle cambios o avisos).
// Así los stores y componentes son los mismos en los dos casos.
import * as tauri from "@tauri-apps/api/core";
import * as eventosTauri from "@tauri-apps/api/event";
import { esJugador } from "$lib/modo";

export type Args = Record<string, unknown>;
type Manejador = (cmd: string, args: Args) => Promise<unknown>;

let manejadorJugador: Manejador | null = null;

/** La conexión del jugador se instala aquí para contestar los comandos. */
export function instalarJugador(m: Manejador) {
  manejadorJugador = m;
}

export function invoke<T>(cmd: string, args: Args = {}): Promise<T> {
  if (!esJugador) return tauri.invoke<T>(cmd, args);
  if (!manejadorJugador) return Promise.reject("Sin conexión con la sala");
  return manejadorJugador(cmd, args) as Promise<T>;
}

/** Ruta del disco del host -> URL del webview (solo tiene sentido en el host). */
export function convertFileSrc(ruta: string): string {
  return tauri.convertFileSrc(ruta);
}

// ------------------------------------------------------------------
// Eventos
// ------------------------------------------------------------------

type Oyente = (e: { payload: unknown }) => void;
const oyentes = new Map<string, Set<Oyente>>();

/** Como `listen` de Tauri: devuelve (en una promesa) cómo dejar de escuchar. */
export async function listen<T>(evento: string, cb: (e: { payload: T }) => void): Promise<() => void> {
  if (!esJugador) return eventosTauri.listen<T>(evento, cb);
  const lista = oyentes.get(evento) ?? new Set<Oyente>();
  oyentes.set(evento, lista);
  const oyente = cb as Oyente;
  lista.add(oyente);
  return () => lista.delete(oyente);
}

/** Emite un evento en esta pestaña (lo usa la conexión del jugador). */
export function emitirLocal(evento: string, payload: unknown) {
  for (const cb of oyentes.get(evento) ?? []) {
    try {
      cb({ payload });
    } catch (err) {
      console.error(`Evento ${evento}:`, err);
    }
  }
}

/** Avisa a los stores de que la partida cambió y deben releer (ver `partida.alCambiarPartida`). */
export const EVENTO_PARTIDA_CAMBIADA = "partida-cambiada";
