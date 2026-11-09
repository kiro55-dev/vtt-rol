// cliente-web/src/enlace.ts
//
// El enlace que pasa el host: `<esta página>#t=<túnel>&s=<sala>&k=<secreto>`. Todo va tras el `#`, que el
// navegador nunca envía a ningún servidor (ni a GitHub Pages). Funciones puras, con tests.

export interface Enlace {
  /** Dirección del túnel del host (`xxxx.trycloudflare.com`). Cambia cada vez que abre la sala. */
  tunel: string;
  /** Identidad de la sala (de una partida). Fija hasta que el host genere un enlace nuevo. */
  sala: string;
  secreto: string;
}

/** Solo túneles de Cloudflare: el enlace no puede mandar el navegador a cualquier sitio. */
const RE_TUNEL = /^[a-z0-9-]+\.trycloudflare\.com$/;
const RE_CLAVE = /^[A-Za-z0-9_-]{16,64}$/;

export function leerEnlace(hash: string): Enlace | null {
  const p = new URLSearchParams(hash.replace(/^#/, ""));
  const tunel = (p.get("t") ?? "").toLowerCase();
  const sala = p.get("s") ?? "";
  const secreto = p.get("k") ?? "";
  if (!RE_TUNEL.test(tunel) || !RE_CLAVE.test(sala) || !RE_CLAVE.test(secreto)) return null;
  return { tunel, sala, secreto };
}

export function urlWebSocket(e: Enlace): string {
  return `wss://${e.tunel}/ws`;
}

/**
 * Una imagen de la partida pedida al host. `mini` = su miniatura ("ficha" o "mapa") en vez del original.
 * El host solo la da si el jugador de esa `sesion` la ve.
 */
export function urlImagen(e: Enlace, sesion: string, ruta: string, mini?: "ficha" | "mapa"): string {
  const q = new URLSearchParams({ ruta, sesion });
  if (mini) q.set("mini", mini);
  return `https://${e.tunel}/imagen?${q}`;
}

/** Dónde guarda el navegador su token de ESTA sala (cada sala, uno). */
export const claveAlmacen = (sala: string) => `openroll.sala.${sala}`;
