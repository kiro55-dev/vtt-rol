// src/lib/red/imagenesRed.ts
//
// Las imágenes del JUGADOR por la misma conexión que el juego (ver src-tauri/src/red/imagenes.rs):
//  * `pedir(ruta, mini)`: manda `PedirImagen`, recibe `ImagenCabecera` y los trozos binarios y devuelve un `Blob`.
//    Como mucho `MAX_A_LA_VEZ` a la vez (el resto espera turno); si el host contesta «ocupado», se reintenta.
//    Lo ya bajado se guarda en la Cache API del navegador: el nombre de cada imagen es el hash de su contenido, así
//    que nunca se queda vieja. Se vacía al salir de la sala.
//  * `subir(archivo, ext)`: `SubirImagen` + trozos binarios, sin llenar el búfer de la conexión.
// No sabe nada del WebSocket: la conexión le pasa cómo enviar y le entrega lo que llega. Así se prueba sola.
import type { MensajeCliente, MensajeHost } from "./protocolo";

/** Bytes de cada trozo (igual que `red::imagenes::TROZO`). */
export const TROZO = 16 * 1024;
/** Primer byte de un trozo binario (igual que `protocolo.rs`). */
export const TROZO_BAJADA = 1;
export const TROZO_SUBIDA = 3;
/** Imágenes pedidas a la vez. */
export const MAX_A_LA_VEZ = 6;
/** Sin nada nuevo de una imagen en este tiempo, se da por perdida. */
export const TIEMPO_SIN_NOTICIAS_MS = 30_000;
const REINTENTOS_SI_OCUPADO = 3;
const ESPERA_SI_OCUPADO_MS = 1000;
const CACHE = "openroll-img";
/** Imágenes guardadas como mucho en la caché (las más viejas se tiran). */
const MAX_EN_CACHE = 500;

/** Un trozo binario: tipo + id (u32, big endian) + datos. */
export function trozo(tipo: number, id: number, datos: Uint8Array): Uint8Array<ArrayBuffer> {
  const t = new Uint8Array(5 + datos.length);
  t[0] = tipo;
  new DataView(t.buffer).setUint32(1, id);
  t.set(datos, 5);
  return t;
}

export function leerTrozo(b: ArrayBuffer): { tipo: number; id: number; datos: Uint8Array } | null {
  if (b.byteLength < 5) return null;
  const v = new DataView(b);
  return { tipo: v.getUint8(0), id: v.getUint32(1), datos: new Uint8Array(b, 5) };
}

/** Dónde vive una imagen en la caché (y la URL que intercepta el Service Worker). */
export function claveImagen(base: string, ruta: string, mini?: string | null): string {
  const q = new URLSearchParams({ ruta });
  if (mini) q.set("mini", mini);
  return `${base}/__imagen?${q}`;
}

interface Envio {
  /** Manda un mensaje del protocolo. */
  mensaje(m: MensajeCliente): void;
  /** Manda un trozo binario. */
  binario(b: Uint8Array<ArrayBuffer>): void;
  /** Se cumple cuando hay sitio para mandar más (el búfer de la conexión no está lleno). */
  sitio(): Promise<void>;
}

interface Bajada {
  ruta: string;
  mini?: string;
  ok: (b: Blob) => void;
  mal: (e: string) => void;
  intentos: number;
  mime?: string;
  tam?: number;
  trozos: Uint8Array[];
  recibidos: number;
  reloj?: ReturnType<typeof setTimeout>;
}

interface Subida {
  ok: (ruta: string) => void;
  mal: (e: string) => void;
}

export class ImagenesRed {
  #envio: Envio;
  #base: string;
  #siguiente = 1;
  #cola: Bajada[] = [];
  #enMarcha = new Map<number, Bajada>();
  #subidas = new Map<number, Subida>();
  /** Pedidas iguales en curso: se comparte la misma promesa. */
  #pedidas = new Map<string, Promise<Blob>>();

  constructor(envio: Envio, base = "") {
    this.#envio = envio;
    this.#base = base;
  }

  /** Una imagen de la partida (`mini`: su miniatura, "ficha" o "mapa"). */
  pedir(ruta: string, mini?: string | null): Promise<Blob> {
    const clave = claveImagen(this.#base, ruta, mini);
    const ya = this.#pedidas.get(clave);
    if (ya) return ya;
    const p = (async () => {
      const guardada = await leerDeCache(clave);
      if (guardada) return guardada;
      const blob = await new Promise<Blob>((ok, mal) => {
        this.#cola.push({ ruta, mini: mini ?? undefined, ok, mal, intentos: 0, trozos: [], recibidos: 0 });
        this.#arrancar();
      });
      void guardarEnCache(clave, blob);
      return blob;
    })();
    this.#pedidas.set(clave, p);
    const olvidar = () => this.#pedidas.delete(clave);
    p.then(olvidar, olvidar);
    return p;
  }

  /** Sube una imagen (sus bytes) y devuelve su ruta en la partida ("imagenes/…"). */
  async subir(datos: Blob, ext: string): Promise<string> {
    const bytes = new Uint8Array(await datos.arrayBuffer());
    const id = this.#siguiente++;
    const hecho = new Promise<string>((ok, mal) => this.#subidas.set(id, { ok, mal }));
    this.#envio.mensaje({ tipo: "SubirImagen", id, ext, tam: bytes.length });
    for (let i = 0; i < bytes.length; i += TROZO) {
      if (!this.#subidas.has(id)) break; // el host ya la rechazó
      await this.#envio.sitio();
      this.#envio.binario(trozo(TROZO_SUBIDA, id, bytes.subarray(i, i + TROZO)));
    }
    return hecho;
  }

  /** Un mensaje del host. Devuelve si era de imágenes. */
  alMensaje(m: MensajeHost): boolean {
    switch (m.tipo) {
      case "ImagenCabecera": {
        const b = this.#enMarcha.get(m.id);
        if (!b) return true;
        b.mime = m.mime;
        b.tam = m.tam;
        this.#vigilar(m.id, b);
        if (m.tam === 0) this.#terminar(m.id, b);
        return true;
      }
      case "ImagenError": {
        const b = this.#enMarcha.get(m.id);
        if (!b) return true;
        this.#soltar(m.id, b);
        if (m.error === "ocupado" && b.intentos < REINTENTOS_SI_OCUPADO) {
          b.intentos++;
          setTimeout(() => {
            this.#cola.unshift(b);
            this.#arrancar();
          }, ESPERA_SI_OCUPADO_MS);
        } else {
          b.mal(m.error);
        }
        this.#arrancar();
        return true;
      }
      case "SubidaOk":
      case "SubidaError": {
        const s = this.#subidas.get(m.id);
        this.#subidas.delete(m.id);
        if (m.tipo === "SubidaOk") s?.ok(m.ruta);
        else s?.mal(m.error);
        return true;
      }
      default:
        return false;
    }
  }

  /** Un trozo binario del host. */
  alBinario(datos: ArrayBuffer) {
    const t = leerTrozo(datos);
    if (!t || t.tipo !== TROZO_BAJADA) return;
    const b = this.#enMarcha.get(t.id);
    if (!b || b.tam === undefined) return;
    b.trozos.push(t.datos.slice());
    b.recibidos += t.datos.length;
    this.#vigilar(t.id, b);
    if (b.recibidos >= b.tam) this.#terminar(t.id, b);
  }

  /** La conexión se cortó: todo lo pendiente falla (al reconectar se vuelve a pedir lo que haga falta). */
  cortar(motivo: string) {
    for (const [id, b] of this.#enMarcha) {
      this.#soltar(id, b);
      b.mal(motivo);
    }
    for (const b of this.#cola.splice(0)) b.mal(motivo);
    for (const s of this.#subidas.values()) s.mal(motivo);
    this.#subidas.clear();
  }

  /** Al salir de la sala: además, se olvida lo guardado en la caché del navegador. */
  async terminar(motivo: string) {
    this.cortar(motivo);
    try {
      await globalThis.caches?.delete(CACHE);
    } catch {
      // Sin Cache API: nada que borrar
    }
  }

  #arrancar() {
    while (this.#enMarcha.size < MAX_A_LA_VEZ && this.#cola.length) {
      const b = this.#cola.shift()!;
      const id = this.#siguiente++;
      b.trozos = [];
      b.recibidos = 0;
      b.tam = undefined;
      this.#enMarcha.set(id, b);
      this.#vigilar(id, b);
      this.#envio.mensaje({ tipo: "PedirImagen", id, ruta: b.ruta, mini: b.mini });
    }
  }

  /** Si no llega nada de la imagen en un rato, se da por perdida (y se le dice al host que pare). */
  #vigilar(id: number, b: Bajada) {
    clearTimeout(b.reloj);
    b.reloj = setTimeout(() => {
      if (this.#enMarcha.get(id) !== b) return;
      this.#soltar(id, b);
      this.#envio.mensaje({ tipo: "CancelarImagen", id });
      b.mal("La imagen no ha llegado");
      this.#arrancar();
    }, TIEMPO_SIN_NOTICIAS_MS);
  }

  #terminar(id: number, b: Bajada) {
    this.#soltar(id, b);
    b.ok(new Blob(b.trozos as BlobPart[], { type: b.mime }));
    this.#arrancar();
  }

  #soltar(id: number, b: Bajada) {
    clearTimeout(b.reloj);
    this.#enMarcha.delete(id);
  }
}

// ------------------------------------------------------------------
// Caché del navegador (Cache API). Si no la hay (pruebas, navegadores raros), simplemente no se guarda.
// ------------------------------------------------------------------

async function leerDeCache(clave: string): Promise<Blob | null> {
  try {
    const r = await (await globalThis.caches?.open(CACHE))?.match(new URL(clave, location.origin));
    return r ? await r.blob() : null;
  } catch {
    return null;
  }
}

async function guardarEnCache(clave: string, blob: Blob) {
  try {
    const c = await globalThis.caches?.open(CACHE);
    if (!c) return;
    await c.put(new URL(clave, location.origin), new Response(blob, { headers: { "Content-Type": blob.type } }));
    const claves = await c.keys();
    for (const viejo of claves.slice(0, Math.max(0, claves.length - MAX_EN_CACHE))) await c.delete(viejo);
  } catch {
    // Cuota llena o sin Cache API: la imagen se volverá a pedir la próxima vez
  }
}
