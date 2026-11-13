// src/lib/red/enlace.ts
//
// El enlace que pasa el host. Todo va tras el `#`, que el navegador nunca envía a ningún servidor:
//  * Con el túnel de Cloudflare: `<página de GitHub Pages>#t=<túnel>&s=<sala>&k=<secreto>[&f=<huella>&r=<relés>]`
//    (`f` y `r`, solo si se ofrece la conexión directa: la huella del host y los relés de Nostr que comprobó).
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
  /** Huella SHA-256 del certificado del host (base64url, 43 caracteres): con ella la conexión directa (WebRTC) sabe
   *  que habla con el host de verdad. `null`: el enlace no ofrece conexión directa (red local, o uno antiguo). */
  huella: string | null;
  /** Los relés de Nostr que el host comprobó (`wss://host`), para ponerse de acuerdo con él. `null`/ausente: el enlace no
   *  los trae (uno antiguo): se usan los de `senal.ts` por defecto. */
  reles?: string[] | null;
}

/**
 * Una dirección con forma de dominio (y puerto opcional): sin rutas, usuarios ni nada que pueda cambiar adónde va
 * la conexión. Ya no se limita a `*.trycloudflare.com` porque el host puede usar su dirección propia; aun así un
 * enlace falso no puede robar nada: la prueba del token va atada a esta dirección (ver `prueba`).
 */
const RE_TUNEL = /^(?=.{1,253}(:\d{1,5})?$)[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?)*(?::\d{1,5})?$/;
const RE_CLAVE = /^[A-Za-z0-9_-]{16,64}$/;
const RE_HUELLA = /^[A-Za-z0-9_-]{43}$/;

/** Como mucho los relés que se admiten en un enlace. */
const MAX_RELES = 6;

/**
 * Los relés del parámetro `r` (`host[:puerto],host…`, siempre por `wss`). Los que no tienen forma de dominio se
 * descartan (un enlace no puede mandar al navegador a una dirección rara). `null`: el enlace no trae `r`.
 */
function leerReles(r: string | null): string[] | null {
  if (r === null) return null;
  return r
    .split(",")
    .map((h) => h.trim().toLowerCase())
    .filter((h) => RE_TUNEL.test(h))
    .slice(0, MAX_RELES)
    .map((h) => `wss://${h}`);
}

/** La huella del enlace como la escribe el SDP: `AA:BB:…` en mayúsculas (32 bytes). */
export function huellaSdp(huella: string): string {
  const b = atob(huella.replace(/-/g, "+").replace(/_/g, "/") + "=");
  return [...b].map((c) => c.charCodeAt(0).toString(16).padStart(2, "0").toUpperCase()).join(":");
}

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
    return { tunel, sala: `lan:${tunel}`, secreto: "", mismoOrigen: true, seguro: ubicacion.protocol === "https:", huella: null };
  }
  if (!RE_CLAVE.test(sala) || !RE_CLAVE.test(secreto)) return null;
  // La huella es opcional (enlaces sin conexión directa), pero si viene tiene que estar bien formada
  const f = p.get("f");
  if (f !== null && !RE_HUELLA.test(f)) return null;
  const huella = f;
  const reles = leerReles(p.get("r"));
  if (t !== null) {
    const tunel = t.toLowerCase();
    if (!RE_TUNEL.test(tunel)) return null;
    return { tunel, sala, secreto, mismoOrigen: false, seguro: true, huella, reles };
  }
  // Sin `t`: el host es quien sirve esta misma página
  if (!ubicacion) return null;
  const tunel = ubicacion.host.toLowerCase();
  if (!RE_TUNEL.test(tunel) || !/^https?:$/.test(ubicacion.protocol)) return null;
  return { tunel, sala, secreto, mismoOrigen: true, seguro: ubicacion.protocol === "https:", huella, reles };
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
