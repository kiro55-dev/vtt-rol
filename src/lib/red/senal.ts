// src/lib/red/senal.ts
//
// Señalización AUTOMÁTICA de la conexión directa por relés públicos de Nostr (espejo de src-tauri/src/red/senal.rs).
// El jugador publica su oferta (el código cifrado de `codigo.ts`) en unos relés y espera la respuesta del host por el
// mismo camino. Los relés solo ven bultos opacos: el contenido va cifrado con el secreto del enlace, la etiqueta con la
// que se encuentran sale de ese secreto, y el navegador rechaza una respuesta cuyo certificado no sea el del enlace.
//
// Eventos efímeros (kind 27184: los relés los reenvían y no los guardan), firmados con una clave Schnorr nueva por
// evento (los relés la exigen; no identifica a nadie).
import { schnorr } from "@noble/curves/secp256k1.js";
import { hmac } from "@noble/hashes/hmac.js";
import { sha256 } from "@noble/hashes/sha2.js";
import { bytesToHex, hexToBytes, utf8ToBytes } from "@noble/hashes/utils.js";
import { descifrar } from "./codigo";

export const KIND = 27184;

/** Relés por los que se intercambian las ofertas. Hay que cambiarlos a la vez en `src-tauri/src/red/senal.rs`. */
export const RELES = ["wss://relay.snort.social", "wss://relay.primal.net", "wss://nostr.oxtr.dev", "wss://offchain.pub"];

/** Lo que se espera la respuesta del host antes de pasar al respaldo. */
export const TOPE_RESPUESTA_MS = 10_000;

export interface Evento {
  id: string;
  pubkey: string;
  created_at: number;
  kind: number;
  tags: string[][];
  content: string;
  sig: string;
}

/** La etiqueta de las ofertas (`"oferta"`) o de las respuestas (`"respuesta"`) de la sala: 16 bytes del HMAC del secreto. */
export function etiqueta(secreto: string, tipo: "oferta" | "respuesta"): string {
  return bytesToHex(hmac(sha256, utf8ToBytes(secreto), utf8ToBytes(`openroll-senal-${tipo}`)).slice(0, 16));
}

function digestId(pubkey: string, createdAt: number, kind: number, tags: string[][], content: string): Uint8Array {
  return sha256(utf8ToBytes(JSON.stringify([0, pubkey, createdAt, kind, tags, content])));
}

/** Crea y firma un evento con `clave` (32 bytes) y la aleatoriedad auxiliar `aux`. */
export function crear(clave: Uint8Array, aux: Uint8Array, createdAt: number, tags: string[][], content: string): Evento {
  const pubkey = bytesToHex(schnorr.getPublicKey(clave));
  const id = digestId(pubkey, createdAt, KIND, tags, content);
  return { id: bytesToHex(id), pubkey, created_at: createdAt, kind: KIND, tags, content, sig: bytesToHex(schnorr.sign(id, clave, aux)) };
}

/** Un evento con una clave aleatoria nueva (la que se usa de verdad). */
export function crearAleatorio(createdAt: number, tags: string[][], content: string): Evento {
  const { secretKey } = schnorr.keygen();
  return crear(secretKey, crypto.getRandomValues(new Uint8Array(32)), createdAt, tags, content);
}

export function idValido(e: Evento): boolean {
  return bytesToHex(digestId(e.pubkey, e.created_at, e.kind, e.tags, e.content)) === e.id;
}

export function firmaValida(e: Evento): boolean {
  try {
    return schnorr.verify(hexToBytes(e.sig), hexToBytes(e.id), hexToBytes(e.pubkey));
  } catch {
    return false;
  }
}

const valorDe = (e: Evento, nombre: string) => e.tags.find((t) => t.length >= 2 && t[0] === nombre)?.[1];

/** Lo que dice un mensaje de un relé, si es un evento: `["EVENT", <suscripción>, <evento>]`. */
export function leerEvento(texto: string): Evento | null {
  if (texto.length > 64 * 1024) return null;
  try {
    const a = JSON.parse(texto);
    if (!Array.isArray(a) || a[0] !== "EVENT" || a.length < 3) return null;
    const e = a[2];
    if (typeof e?.id !== "string" || typeof e.pubkey !== "string" || typeof e.content !== "string" || !Array.isArray(e.tags)) return null;
    return e as Evento;
  } catch {
    return null;
  }
}

export interface OpcionesPedir {
  secreto: string;
  /** Mi oferta, ya cifrada (el código de `codigo.ts`). */
  codigoOferta: string;
  /** El `n` de la oferta: la respuesta del host lo lleva en su etiqueta `x`. */
  n: string;
  reles?: string[];
  topeMs?: number;
  /** Para los tests. */
  crearWebSocket?: (url: string) => WebSocket;
  /** Para cortar la espera (p. ej. si el jugador elige otro camino). */
  cancelar?: AbortSignal;
}

/**
 * Publica la oferta en los relés y devuelve el código de la respuesta del host (la primera que llegue y sea para esta
 * oferta). Lanza un texto si se agota el tiempo o no hay ningún relé. El código aún hay que validarlo (huella).
 */
export function pedirRespuesta(o: OpcionesPedir): Promise<string> {
  const reles = o.reles ?? RELES;
  const abrir = o.crearWebSocket ?? ((u: string) => new WebSocket(u));
  const etiquetaRespuesta = etiqueta(o.secreto, "respuesta");
  const oferta = crearAleatorio(Math.floor(Date.now() / 1000), [["t", etiqueta(o.secreto, "oferta")]], o.codigoOferta);
  const sockets: WebSocket[] = [];

  return new Promise<string>((ok, mal) => {
    let acabado = false;
    const terminar = (f: () => void) => {
      if (acabado) return;
      acabado = true;
      clearTimeout(reloj);
      o.cancelar?.removeEventListener("abort", alCancelar);
      for (const s of sockets) {
        try {
          s.close();
        } catch {
          // ya cerrado
        }
      }
      f();
    };
    const alCancelar = () => terminar(() => mal("Cancelado"));
    const reloj = setTimeout(() => terminar(() => mal("El host no ha contestado")), o.topeMs ?? TOPE_RESPUESTA_MS);
    if (o.cancelar?.aborted) return alCancelar();
    o.cancelar?.addEventListener("abort", alCancelar);

    for (const url of reles) {
      let ws: WebSocket;
      try {
        ws = abrir(url);
      } catch {
        continue;
      }
      sockets.push(ws);
      ws.onopen = () => {
        // Primero escucha la respuesta y luego publica la oferta (así no se pierde una respuesta rápida)
        ws.send(JSON.stringify(["REQ", "r", { kinds: [KIND], "#t": [etiquetaRespuesta], "#x": [o.n], since: Math.floor(Date.now() / 1000) - 30 }]));
        ws.send(JSON.stringify(["EVENT", oferta]));
      };
      ws.onmessage = (m) => {
        if (acabado || typeof m.data !== "string") return;
        const e = leerEvento(m.data);
        if (!e || e.kind !== KIND || valorDe(e, "t") !== etiquetaRespuesta || valorDe(e, "x") !== o.n || !idValido(e)) return;
        // Solo vale si descifra con el secreto de la sala y es una respuesta (el relé no lo puede fabricar)
        descifrar(o.secreto, e.content).then(
          (c) => {
            if (c.tipo === "respuesta") terminar(() => ok(e.content));
          },
          () => {}
        );
      };
      ws.onerror = () => {};
    }
    if (sockets.length === 0) terminar(() => mal("No hay relés a los que conectar"));
  });
}
