// src/lib/red/enlace.ts
//
// El enlace que pasa el host. Todo va tras el `#`, que el navegador nunca envía a ningún servidor:
//  * Con el túnel de Cloudflare: `<página de GitHub Pages>#t=<túnel>&s=<sala>&k=<secreto>`.
//  * Con la dirección propia, la página la sirve la propia app del host y el enlace es su dirección:
//    `https://mi.dominio.es/#s=…&k=…`. Sin `t`: el host es el de la página («mismo origen»).
//  * En red local (como «abrir en LAN»): `http://26.1.2.3:8080/`, sin nada más. Sin llave: la confianza está en la
//    red. El host solo la deja pasar en ese modo (`SalaAbierta::sin_llave`); en los demás, un enlace sin `s`/`k` no
//    entra. Va por `http`/`ws`, sin cifrar.
// Funciones puras, con tests.

export interface Enlace {
  /** Dirección por la que se llega al host: su túnel de Cloudflare (`xxxx.trycloudflare.com`, cambia cada vez que
   *  abre la sala), su dirección propia (`mi.dominio.es`) o, en red local, `ip:puerto`. */
  tunel: string;
  /** Identidad de la sala (de una partida). Fija hasta que el host genere un enlace nuevo. */
  sala: string;
  secreto: string;
  /** La página la sirve el propio host (el enlace no traía `t`): las peticiones van a la misma dirección. */
  mismoOrigen: boolean;
  /** `https`/`wss`. Solo es `false` en red local (página servida por `http`). */
  seguro: boolean;
}

/**
 * Una dirección con forma de dominio (y puerto opcional): sin rutas, usuarios ni nada que pueda cambiar adónde va
 * la conexión. Ya no se limita a `*.trycloudflare.com` porque el host puede usar su dirección propia; aun así un
 * enlace falso no puede robar nada: la prueba del token va atada a esta dirección (ver `prueba`).
 */
const RE_TUNEL = /^(?=.{1,253}(:\d{1,5})?$)[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?)*(?::\d{1,5})?$/;
const RE_CLAVE = /^[A-Za-z0-9_-]{16,64}$/;

/** Dónde está la página (`location`), para el enlace sin `t`. */
export interface Ubicacion {
  host: string;
  protocol: string;
}

export function leerEnlace(hash: string, ubicacion?: Ubicacion): Enlace | null {
  const p = new URLSearchParams(hash.replace(/^#/, ""));
  const sala = p.get("s") ?? "";
  const secreto = p.get("k") ?? "";
  const t = p.get("t");
  // Sin llave (red local): solo con la página del propio host. Se recuerda al jugador por esa dirección
  if (t === null && !sala && !secreto && ubicacion) {
    const tunel = ubicacion.host.toLowerCase();
    if (!RE_TUNEL.test(tunel) || !/^https?:$/.test(ubicacion.protocol)) return null;
    return { tunel, sala: `lan:${tunel}`, secreto: "", mismoOrigen: true, seguro: ubicacion.protocol === "https:" };
  }
  if (!RE_CLAVE.test(sala) || !RE_CLAVE.test(secreto)) return null;
  if (t !== null) {
    const tunel = t.toLowerCase();
    if (!RE_TUNEL.test(tunel)) return null;
    return { tunel, sala, secreto, mismoOrigen: false, seguro: true };
  }
  // Sin `t`: el host es quien sirve esta misma página
  if (!ubicacion) return null;
  const tunel = ubicacion.host.toLowerCase();
  if (!RE_TUNEL.test(tunel) || !/^https?:$/.test(ubicacion.protocol)) return null;
  return { tunel, sala, secreto, mismoOrigen: true, seguro: ubicacion.protocol === "https:" };
}

export function urlWebSocket(e: Enlace): string {
  return `${e.seguro ? "wss" : "ws"}://${e.tunel}/ws`;
}

/**
 * Una imagen de la partida pedida al host por HTTP (el respaldo sin Service Worker: con él van por la conexión).
 * `mini` = su miniatura ("ficha" o "mapa") en vez del original. El host solo la da si el jugador de esa `sesion` la
 * ve. Si la página la sirve el host, la dirección es relativa (mismo origen, sin CORS).
 */
export function urlImagen(e: Enlace, sesion: string, ruta: string, mini?: "ficha" | "mapa"): string {
  const q = new URLSearchParams({ ruta, sesion });
  if (mini) q.set("mini", mini);
  return e.mismoOrigen ? `/imagen?${q}` : `https://${e.tunel}/imagen?${q}`;
}

/** Dónde guarda el navegador su token de ESTA sala (cada sala, uno). */
export const claveAlmacen = (sala: string) => `openroll.sala.${sala}`;
