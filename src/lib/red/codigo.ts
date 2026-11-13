// src/lib/red/codigo.ts
//
// Los códigos de la conexión directa a mano (espejo de src-tauri/src/red/codigo.rs): la oferta del jugador y la
// respuesta del host, cifradas con una clave sacada del secreto del enlace (el SDP lleva las IPs):
//   código = base64url( nonce (12 bytes) ‖ AES-256-GCM( deflate-raw( JSON { tipo, sdp, t, n } ) ) )
//   clave  = HKDF-SHA256(secreto, info = "openroll-codigo")
// Usa WebCrypto: la conexión directa solo se intenta desde una página https (contexto seguro).

export interface Contenido {
  tipo: "oferta" | "respuesta";
  sdp: string;
  /** Cuándo se hizo (milisegundos Unix). */
  t: number;
  /** Aleatorio: el host no acepta dos veces el mismo. */
  n: string;
}

const MAX_CODIGO = 16 * 1024;
const MAX_CONTENIDO = 64 * 1024;
const texto = new TextEncoder();

async function clave(secreto: string): Promise<CryptoKey> {
  const base = await crypto.subtle.importKey("raw", texto.encode(secreto), "HKDF", false, ["deriveKey"]);
  return crypto.subtle.deriveKey(
    { name: "HKDF", hash: "SHA-256", salt: new Uint8Array(0), info: texto.encode("openroll-codigo") },
    base,
    { name: "AES-GCM", length: 256 },
    false,
    ["encrypt", "decrypt"]
  );
}

async function transformar(datos: Uint8Array<ArrayBuffer>, t: CompressionStream | DecompressionStream, max = Infinity): Promise<Uint8Array> {
  const lector = new Blob([datos]).stream().pipeThrough(t).getReader();
  const partes: Uint8Array[] = [];
  let total = 0;
  for (;;) {
    const { done, value } = await lector.read();
    if (done) break;
    total += value.length;
    if (total > max) throw "El código está dañado";
    partes.push(value);
  }
  const fuera = new Uint8Array(total);
  let i = 0;
  for (const p of partes) {
    fuera.set(p, i);
    i += p.length;
  }
  return fuera;
}

const aBase64url = (b: Uint8Array) => btoa(String.fromCharCode(...b)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
function deBase64url(s: string): Uint8Array<ArrayBuffer> {
  const b = atob(s.replace(/-/g, "+").replace(/_/g, "/"));
  return Uint8Array.from(b, (c) => c.charCodeAt(0));
}

/** Cifra un contenido con el secreto de la sala. `nonce` solo se pasa en los tests. */
export async function cifrar(secreto: string, contenido: Contenido, nonce?: Uint8Array<ArrayBuffer>): Promise<string> {
  const n = nonce ?? crypto.getRandomValues(new Uint8Array(12));
  const comprimido = await transformar(texto.encode(JSON.stringify(contenido)), new CompressionStream("deflate-raw"));
  const cifrado = new Uint8Array(await crypto.subtle.encrypt({ name: "AES-GCM", iv: n }, await clave(secreto), comprimido as Uint8Array<ArrayBuffer>));
  const todo = new Uint8Array(12 + cifrado.length);
  todo.set(n);
  todo.set(cifrado, 12);
  return aBase64url(todo);
}

/** Descifra un código. Lanza un texto para enseñar si no es de esta sala o está dañado. */
export async function descifrar(secreto: string, codigo: string): Promise<Contenido> {
  const limpio = codigo.trim();
  if (limpio.length > MAX_CODIGO) throw "El código es demasiado largo";
  let todo: Uint8Array<ArrayBuffer>;
  try {
    todo = deBase64url(limpio);
  } catch {
    throw "Eso no es un código válido";
  }
  if (todo.length < 12 + 16) throw "Eso no es un código válido";
  let comprimido: ArrayBuffer;
  try {
    comprimido = await crypto.subtle.decrypt({ name: "AES-GCM", iv: todo.slice(0, 12) }, await clave(secreto), todo.slice(12));
  } catch {
    throw "Ese código no es de esta sala (o está incompleto)";
  }
  try {
    const json = await transformar(new Uint8Array(comprimido), new DecompressionStream("deflate-raw"), MAX_CONTENIDO);
    const c = JSON.parse(new TextDecoder().decode(json));
    if ((c.tipo !== "oferta" && c.tipo !== "respuesta") || typeof c.sdp !== "string") throw 0;
    return c as Contenido;
  } catch {
    throw "El código está dañado";
  }
}

/** Un nonce aleatorio para `Contenido.n`. */
export const nonceAleatorio = () => aBase64url(crypto.getRandomValues(new Uint8Array(12)));
