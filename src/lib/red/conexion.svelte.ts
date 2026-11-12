// src/lib/red/conexion.svelte.ts
//
// La conexión del JUGADOR con la sala del host (ver src-tauri/src/red/protocolo.rs y docs/red.md):
//  1. El host manda un reto. Si este navegador ya entró en la sala, responde con
//     HMAC-SHA256(SHA-256(token), "<reto>|<túnel>"): demuestra que tiene el token sin enviarlo, así un enlace
//     falso que lleve a otro servidor no puede robarlo.
//  2. Si el host no lo reconoce, pide nombre (y contraseña, si la hay). Al entrar, da un token que se guarda
//     en este navegador (localStorage de la página fija) para la próxima vez.
//  3. Después llega la vista entera y luego solo lo que cambia. Con ella se contestan los comandos de la app
//     ($lib/backend), y cada cambio se avisa con el evento `partida-cambiada` para que los stores relean.
// Si se corta, se reintenta durante un minuto (p. ej. el móvil cambia de red).
//
// Imágenes: van por esta misma conexión (`ImagenesRed`). Los `<img>` de la app piden `<base>/__imagen?ruta=…`,
// que contesta el Service Worker (src/service-worker.ts) preguntándole a esta página. Sin Service Worker (p. ej.
// Firefox en ventana privada) se usa el respaldo HTTP de siempre (`/imagen?sesion=…` del túnel).
import { hmac } from "@noble/hashes/hmac.js";
import { sha256 } from "@noble/hashes/sha2.js";
import { bytesToHex } from "@noble/hashes/utils.js";
import { emitirLocal, EVENTO_PARTIDA_CAMBIADA, instalarJugador } from "$lib/backend";
import { EVENTO_AVISO } from "$lib/avisosTablero";
import { esJugador } from "$lib/modo";
import { claveAlmacen, leerEnlace, urlImagen, urlWebSocket, type Enlace } from "./enlace";
import { ImagenesRed, claveImagen } from "./imagenesRed";
import { VERSION_PROTOCOLO, type Jugador, type MensajeCliente, type MensajeHost, type VistaJugador } from "./protocolo";
import { aplicarCambios, partesCambiadas, PETICIONES, responder, separarMiniatura, vistaVacia } from "./replica";

/** Carpeta de la página (`/vtt-rol` en GitHub Pages, vacía en desarrollo): donde vive el Service Worker. */
const BASE = typeof location === "undefined" ? "" : new URL(".", location.href).pathname.replace(/\/$/, "");
/** Lo que se deja en el búfer del WebSocket al subir una imagen antes de esperar. */
const MAX_BUFER_SUBIDA = 1024 * 1024;
/** Lo que se espera a que el Service Worker tome el control de la página antes de usar el respaldo HTTP. */
const ESPERA_SW_MS = 3000;

export type EstadoConexion =
  | { tipo: "conectando" }
  | { tipo: "identificate"; pideContrasena: boolean; error: string | null; recordado: boolean }
  | { tipo: "dentro" }
  | { tipo: "reconectando" }
  | { tipo: "fuera"; motivo: string };

const REINTENTAR_DURANTE_MS = 60_000;
/** Lo que se espera la respuesta del host a una acción. */
const TIEMPO_RESPUESTA_MS = 30_000;
const MAX_COMANDOS_PERSONALES = 100;

/**
 * Lo mismo que `red::sala::prueba` en Rust (con el vector fijo de los tests de los dos lados):
 * HMAC-SHA256(SHA-256(token), "<reto>|<dirección>"). Con `@noble/hashes` y no con `crypto.subtle`, porque en red
 * local la página va por `http://` y ahí el navegador no da `crypto.subtle` (solo en «contextos seguros»).
 */
export async function prueba(token: string, reto: string, tunel: string): Promise<string> {
  const texto = new TextEncoder();
  return bytesToHex(hmac(sha256, sha256(texto.encode(token)), texto.encode(`${reto}|${tunel}`)));
}

function leerToken(sala: string): string | null {
  try {
    return localStorage.getItem(claveAlmacen(sala));
  } catch {
    return null;
  }
}

function guardarToken(sala: string, token: string) {
  try {
    localStorage.setItem(claveAlmacen(sala), token);
  } catch {
    // Sin almacenamiento (ventana privada): la próxima vez se volverá a preguntar
  }
}

export class Conexion {
  estado = $state<EstadoConexion>({ tipo: "conectando" });
  yo = $state<Jugador | null>(null);
  /** Lo que ve este jugador. La app lo lee a través de `invoke` ($lib/backend). */
  vista: VistaJugador = vistaVacia();
  /** Para pedir imágenes al host (la misma mientras dure la sala, también al reconectar). */
  sesion = $state<string | null>(null);

  readonly enlace: Enlace;
  /** Los comandos de dados que ha escrito este jugador (flechas ↑ ↓ del chat). Solo en esta pestaña. */
  #personales: string[] = [];
  /** Acciones enviadas al host que aún no han tenido respuesta. */
  #pendientes = new Map<number, { ok: (v: unknown) => void; error: (e: unknown) => void; reloj: ReturnType<typeof setTimeout> }>();
  #siguiente = 1;
  #ws: WebSocket | null = null;
  #cortadoDesde: number | null = null;
  #final = false;
  #alPrimeraVista: (() => void) | null = null;
  /** Se cumple cuando llega la primera vista y ya se sabe cómo se piden las imágenes (la app puede empezar). */
  readonly lista: Promise<void>;
  /** Imágenes por esta conexión (pedir y subir). */
  readonly imagenes: ImagenesRed;
  /** Si el Service Worker controla la página: las imágenes van por la conexión. Si no, por el HTTP del túnel. */
  #porServiceWorker = false;

  constructor(enlace: Enlace) {
    this.enlace = enlace;
    this.imagenes = new ImagenesRed(
      {
        mensaje: (m) => this.#enviar(m),
        binario: (b) => {
          if (this.#ws?.readyState === WebSocket.OPEN) this.#ws.send(b);
        },
        sitio: () => this.#sitio()
      },
      BASE
    );
    const vista = new Promise<void>((r) => (this.#alPrimeraVista = r));
    const sw = this.#prepararServiceWorker().then((si) => {
      this.#porServiceWorker = si;
    });
    this.lista = Promise.all([vista, sw]).then(() => {});
  }

  /** Registra el Service Worker de las imágenes y espera (poco) a que controle la página. `false` si no hay. */
  async #prepararServiceWorker(): Promise<boolean> {
    const sw = globalThis.navigator?.serviceWorker;
    if (!sw) return false;
    try {
      sw.addEventListener("message", (e) => this.#imagenParaElServiceWorker(e));
      await sw.register(`${BASE}/service-worker.js`, { scope: `${BASE}/`, type: import.meta.env.DEV ? "module" : "classic" });
      await sw.ready;
      if (sw.controller) return true;
      return await new Promise<boolean>((ok) => {
        const reloj = setTimeout(() => ok(false), ESPERA_SW_MS);
        sw.addEventListener("controllerchange", () => {
          clearTimeout(reloj);
          ok(true);
        }, { once: true });
      });
    } catch (err) {
      console.warn("Sin Service Worker: las imágenes irán por HTTP", err);
      return false;
    }
  }

  /** El Service Worker pide una imagen (la de un `<img>` de esta página): se saca por la conexión. */
  #imagenParaElServiceWorker(e: MessageEvent) {
    const d = e.data;
    const puerto = e.ports[0];
    if (d?.tipo !== "openroll-imagen" || !puerto || typeof d.ruta !== "string") return;
    this.imagenes.pedir(d.ruta, typeof d.mini === "string" ? d.mini : null).then(
      (blob) => puerto.postMessage({ ok: true, blob }),
      (err) => puerto.postMessage({ ok: false, error: String(err) })
    );
  }

  /** Se cumple cuando el WebSocket tiene sitio para más (al subir una imagen). */
  async #sitio(): Promise<void> {
    while (this.#ws && this.#ws.readyState === WebSocket.OPEN && this.#ws.bufferedAmount > MAX_BUFER_SUBIDA) {
      await new Promise((r) => setTimeout(r, 20));
    }
    if (this.#ws?.readyState !== WebSocket.OPEN) throw "Sin conexión con la sala";
  }

  /**
   * Lo que la app pide con `invoke`: las lecturas se contestan con la réplica; las acciones de la lista blanca
   * se piden al host, que comprueba que puede hacerlas y las ejecuta en su Rust.
   */
  responder(cmd: string, args: Record<string, unknown>): Promise<unknown> {
    // El foco de un jugador es suyo: no se envía a nadie (ni al host)
    if (cmd === "avisar_tablero" && "Foco" in ((args.aviso as object) ?? {})) return Promise.resolve(null);
    if (PETICIONES.has(cmd)) {
      if (cmd === "tirar_dado") this.#recordarComando(String(args.comando ?? ""));
      return this.pedir({ cmd, ...args });
    }
    try {
      return Promise.resolve(responder(cmd, args, this.vista, this.#personales));
    } catch (err) {
      return Promise.reject(err);
    }
  }

  /** Recupera del host los comandos de dados de este jugador (se guardan en su disco, uno por jugador). */
  async #cargarPersonales() {
    try {
      const guardados = await this.pedir({ cmd: "obtener_historial_personal" });
      if (!Array.isArray(guardados)) return;
      const nuevos = this.#personales.filter((c) => !guardados.includes(c)); // escritos mientras llegaba
      this.#personales = [...guardados.map(String), ...nuevos].slice(-MAX_COMANDOS_PERSONALES);
    } catch {
      // Sin historial previo: se empieza con lo que escriba ahora
    }
  }

  #recordarComando(c: string) {
    const limpio = c.trim();
    if (!limpio || this.#personales.at(-1) === limpio) return;
    this.#personales.push(limpio);
    if (this.#personales.length > MAX_COMANDOS_PERSONALES) this.#personales.shift();
  }

  /** Pide una acción al host y espera su respuesta (el error del host llega tal cual, como texto). */
  pedir(peticion: Record<string, unknown>): Promise<unknown> {
    if (this.estado.tipo !== "dentro" || this.#ws?.readyState !== WebSocket.OPEN) {
      return Promise.reject("Sin conexión con la sala");
    }
    const id = this.#siguiente++;
    return new Promise((ok, error) => {
      const reloj = setTimeout(() => {
        this.#pendientes.delete(id);
        error("El host no ha contestado");
      }, TIEMPO_RESPUESTA_MS);
      this.#pendientes.set(id, { ok, error, reloj });
      this.#enviar({ tipo: "Pedir", id, peticion });
    });
  }

  /** Sube la foto de una de sus fichas al host (por la conexión). Devuelve la ruta ("imagenes/…") para la ficha. */
  async subirImagen(archivo: File): Promise<string> {
    if (this.estado.tipo !== "dentro") throw "Sin conexión con la sala";
    const ext = /\.([A-Za-z0-9]+)$/.exec(archivo.name)?.[1] ?? "";
    return this.imagenes.subir(archivo, ext);
  }

  /** Al cortarse la conexión, lo que esperaba respuesta falla (el host ya no la va a dar). */
  #fallarPendientes(motivo: string) {
    for (const { error, reloj } of this.#pendientes.values()) {
      clearTimeout(reloj);
      error(motivo);
    }
    this.#pendientes.clear();
  }

  /** URL de una imagen de la partida (la ruta puede llevar la marca de miniatura de `obtener_miniatura`). */
  urlImagen(ref: string): string {
    const { ruta, mini } = separarMiniatura(ref);
    if (this.#porServiceWorker) return claveImagen(BASE, ruta, mini);
    if (!this.sesion) return "";
    return urlImagen(this.enlace, this.sesion, ruta, mini);
  }

  conectar() {
    this.#final = false;
    const ws = new WebSocket(urlWebSocket(this.enlace));
    ws.binaryType = "arraybuffer";
    this.#ws = ws;
    ws.onmessage = (e) => {
      // Binario: un trozo de imagen
      if (typeof e.data !== "string") {
        if (e.data instanceof ArrayBuffer) this.imagenes.alBinario(e.data);
        return;
      }
      let m: MensajeHost;
      try {
        m = JSON.parse(e.data);
      } catch {
        return;
      }
      if (this.imagenes.alMensaje(m)) return;
      void this.#recibir(m);
    };
    ws.onclose = () => {
      if (this.#ws !== ws) return;
      this.#fallarPendientes("Sin conexión con la sala");
      this.imagenes.cortar("Sin conexión con la sala");
      if (this.#final) return;
      this.#ws = null;
      this.#reintentar();
    };
  }

  /** Nombre y contraseña, cuando el host los pide. */
  unirse(nombre: string, contrasena: string) {
    this.#enviar({ tipo: "Unirse", nombre: nombre.trim(), contrasena: contrasena || undefined });
  }

  cerrar() {
    this.#final = true;
    this.#ws?.close();
  }

  #enviar(m: MensajeCliente) {
    if (this.#ws?.readyState === WebSocket.OPEN) this.#ws.send(JSON.stringify(m));
  }

  #terminar(motivo: string) {
    this.#final = true;
    this.estado = { tipo: "fuera", motivo };
    void this.imagenes.terminar(motivo);
    this.#ws?.close();
  }

  #reintentar() {
    const ahora = Date.now();
    this.#cortadoDesde ??= ahora;
    if (ahora - this.#cortadoDesde > REINTENTAR_DURANTE_MS) {
      this.estado = { tipo: "fuera", motivo: "Se ha perdido la conexión con el host" };
      return;
    }
    this.estado = { tipo: "reconectando" };
    setTimeout(() => this.conectar(), 2000);
  }

  async #recibir(m: MensajeHost) {
    switch (m.tipo) {
      case "Reto": {
        const token = leerToken(this.enlace.sala);
        const p = token ? await prueba(token, m.reto, this.enlace.tunel) : undefined;
        this.#enviar({ tipo: "Hola", version: VERSION_PROTOCOLO, sala: this.enlace.sala, secreto: this.enlace.secreto, prueba: p });
        return;
      }
      case "Identificate":
        // `recordado`: este navegador ya había entrado y aun así se le pregunta. Puede ser que el host
        // generó un enlace nuevo... o que este enlace no lleva al host de verdad (ver EntradaSala.svelte).
        this.estado = { tipo: "identificate", pideContrasena: m.pide_contrasena, error: m.error ?? null, recordado: !!leerToken(this.enlace.sala) };
        return;
      case "Bienvenido":
        if (m.token) guardarToken(this.enlace.sala, m.token);
        this.yo = m.yo;
        this.sesion = m.sesion;
        this.#cortadoDesde = null;
        this.estado = { tipo: "dentro" };
        void this.#cargarPersonales(); // sus comandos de dados anteriores (↑ ↓), guardados en el host
        return;
      case "Vista": {
        const primera = this.#alPrimeraVista;
        this.vista = m.vista;
        if (primera) {
          this.#alPrimeraVista = null;
          primera();
        } else {
          // Al reconectar: todo puede haber cambiado
          emitirLocal(EVENTO_PARTIDA_CAMBIADA, { zonas: true, mapas: true, entidades: true, definiciones: true, tiradas: true });
        }
        return;
      }
      case "Cambios":
        this.vista = aplicarCambios(this.vista, m.cambios);
        emitirLocal(EVENTO_PARTIDA_CAMBIADA, partesCambiadas(m.cambios));
        return;
      case "Aviso":
        // El tablero ya escucha este evento (ping y recorrido del host y de los demás jugadores)
        emitirLocal(EVENTO_AVISO, m.aviso);
        return;
      case "Respuesta": {
        const p = this.#pendientes.get(m.id);
        if (!p) return;
        this.#pendientes.delete(m.id);
        clearTimeout(p.reloj);
        if (m.error !== undefined) p.error(m.error);
        else p.ok(m.ok ?? null);
        return;
      }
      case "Rechazado":
        return this.#terminar(m.motivo);
      case "SalaCerrada":
        return this.#terminar("El host ha cerrado la sala");
    }
  }
}

/**
 * La conexión de esta pestaña, si es la de un jugador con un enlace válido (`null` en el host o sin enlace).
 * Se crea al cargar el módulo e instala su respuesta en `invoke`.
 */
export const conexion: Conexion | null = (() => {
  if (!esJugador) return null;
  const enlace = leerEnlace(location.hash, location);
  if (!enlace) return null;
  const c = new Conexion(enlace);
  instalarJugador((cmd, args) => c.responder(cmd, args));
  return c;
})();
