// src/lib/red/protocolo.ts
//
// Espejo a mano de src-tauri/src/red/protocolo.rs y red/vista.rs. Si cambia algo allí, cámbialo aquí y
// sube VERSION_PROTOCOLO en los dos lados (y publica la página a la vez que la app).
import type { AvisoTablero } from "$lib/avisosTablero";
import type { DefinicionParametro, Entidad, Instancia, Mapa, Zona } from "$lib/partida.svelte";

export const VERSION_PROTOCOLO = 8;

export interface Jugador {
  id: number;
  nombre: string;
}

/** Una tirada del chat (`Tiradas` en Rust). `autor` es el nombre de quien tiró, vacío si fue el host. */
export interface TiradaChat {
  total: number;
  comando: string;
  hora: string;
  jugador: number;
  autor: string;
}

/** `red::vista::VistaJugador`: lo que ve este jugador de la partida (lo mismo que lee la app, filtrado). */
export interface VistaJugador {
  zonas: Zona[];
  mapas: Mapa[];
  instancias: Instancia[];
  entidades: Entidad[];
  definiciones: DefinicionParametro[];
  /** El chat de tiradas, de todos, la más reciente primero. */
  historial_tiradas: TiradaChat[];
  /** El mapa que debe tener abierto (el del host, si lo comparte). `null`/ausente: aún ninguno. */
  mapa_visible?: number | null;
}

/** `red::vista::Cambios`: lo que no aparece, sigue igual. */
export interface Cambios {
  zonas?: Zona[];
  zonas_quitadas?: number[];
  mapas?: Mapa[];
  mapas_quitados?: number[];
  instancias?: Instancia[];
  instancias_quitadas?: number[];
  entidades?: Entidad[];
  entidades_quitadas?: number[];
  definiciones?: DefinicionParametro[];
  historial_tiradas?: TiradaChat[];
  /** Cambió el mapa que debe ver (`mapa_visible` puede venir vacío: ninguno). */
  mapa_visible_cambiado?: boolean;
  mapa_visible?: number | null;
}

export type MensajeCliente =
  | { tipo: "Hola"; version: number; sala: string; secreto: string; prueba?: string }
  /** ANTES del saludo: la oferta de la conexión directa (el código cifrado de `codigo.ts`). Una por conexión. */
  | { tipo: "OfertaDirecta"; codigo: string }
  | { tipo: "Unirse"; nombre: string; contrasena?: string }
  /** Una acción del jugador: `peticion` es `{ cmd, ...args }` (un comando de la lista blanca del host). */
  | { tipo: "Pedir"; id: number; peticion: Record<string, unknown> }
  /** Una imagen de la partida por esta misma conexión (ver imagenesRed.ts). `mini`: "ficha"/"mapa". */
  | { tipo: "PedirImagen"; id: number; ruta: string; mini?: string }
  | { tipo: "CancelarImagen"; id: number }
  /** Va a subir `tam` bytes en trozos binarios. */
  | { tipo: "SubirImagen"; id: number; ext: string; tam: number }
  /** Un aviso del tablero sin respuesta (el arrastre en vivo): por la conexión directa va por el canal sin reintentos. */
  | { tipo: "Aviso"; aviso: AvisoTablero };

export type MensajeHost =
  | { tipo: "Reto"; reto: string }
  | { tipo: "Rechazado"; motivo: string }
  | { tipo: "Identificate"; pide_contrasena: boolean; error?: string }
  | { tipo: "Bienvenido"; yo: Jugador; token?: string; sesion: string }
  | { tipo: "Vista"; vista: VistaJugador }
  | { tipo: "Cambios"; cambios: Cambios }
  | { tipo: "Aviso"; aviso: AvisoTablero }
  /** La respuesta a un `Pedir`, después de los `Cambios` que provocó. */
  | { tipo: "Respuesta"; id: number; ok?: unknown; error?: string }
  | { tipo: "SalaCerrada" }
  | { tipo: "ImagenCabecera"; id: number; mime: string; tam: number }
  | { tipo: "ImagenError"; id: number; error: string }
  | { tipo: "SubidaOk"; id: number; ruta: string }
  | { tipo: "SubidaError"; id: number; error: string }
  /** La respuesta del host a `OfertaDirecta`: su código cifrado (con la huella de su certificado). */
  | { tipo: "RespuestaDirecta"; codigo: string }
  | { tipo: "OfertaDirectaError"; error: string };
