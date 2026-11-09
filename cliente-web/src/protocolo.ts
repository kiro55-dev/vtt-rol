// cliente-web/src/protocolo.ts
//
// Espejo a mano de src-tauri/src/red/protocolo.rs y red/vista.rs. Si cambia algo allí, cámbialo aquí y
// sube VERSION_PROTOCOLO en los dos lados.
import type { AvisoTablero } from "$lib/avisosTablero";
import type { DefinicionParametro, Entidad, Instancia, Mapa } from "$lib/partida.svelte";

export const VERSION_PROTOCOLO = 1;

export interface Jugador {
  id: number;
  nombre: string;
}

/** `red::vista::VistaJugador`: lo que ve este jugador de la partida. */
export interface VistaJugador {
  mapa: Mapa | null;
  instancias: Instancia[];
  entidades: Entidad[];
  definiciones: DefinicionParametro[];
}

/** `red::vista::Cambios`: lo que no aparece, sigue igual. */
export interface Cambios {
  mapa?: { actual: Mapa | null };
  instancias?: Instancia[];
  instancias_quitadas?: number[];
  entidades?: Entidad[];
  entidades_quitadas?: number[];
  definiciones?: DefinicionParametro[];
}

export type MensajeCliente =
  | { tipo: "Hola"; version: number; sala: string; secreto: string; prueba?: string }
  | { tipo: "Unirse"; nombre: string; contrasena?: string };

export type MensajeHost =
  | { tipo: "Reto"; reto: string }
  | { tipo: "Rechazado"; motivo: string }
  | { tipo: "Identificate"; pide_contrasena: boolean; error?: string }
  | { tipo: "Bienvenido"; yo: Jugador; token?: string; sesion: string }
  | { tipo: "Vista"; vista: VistaJugador }
  | { tipo: "Cambios"; cambios: Cambios }
  | { tipo: "Aviso"; aviso: AvisoTablero }
  | { tipo: "SalaCerrada" };
