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
import { emitirLocal, EVENTO_PARTIDA_CAMBIADA, instalarJugador } from "$lib/backend";
import { EVENTO_AVISO } from "$lib/avisosTablero";
import { esJugador } from "$lib/modo";
import { claveAlmacen, leerEnlace, urlImagen, urlWebSocket, type Enlace } from "./enlace";
import { VERSION_PROTOCOLO, type Jugador, type MensajeCliente, type MensajeHost, type VistaJugador } from "./protocolo";
import { aplicarCambios, partesCambiadas, responder, separarMiniatura, vistaVacia } from "./replica";

export type EstadoConexion =
  | { tipo: "conectando" }
  | { tipo: "identificate"; pideContrasena: boolean; error: string | null; recordado: boolean }
  | { tipo: "dentro" }
  | { tipo: "reconectando" }
  | { tipo: "fuera"; motivo: string };

const REINTENTAR_DURANTE_MS = 60_000;

const hex = (b: ArrayBuffer) => [...new Uint8Array(b)].map((x) => x.toString(16).padStart(2, "0")).join("");

/** Lo mismo que `red::sala::prueba` en Rust (con el vector fijo de los tests de los dos lados). */
export async function prueba(token: string, reto: string, tunel: string): Promise<string> {
  const c = globalThis.crypto.subtle;
  const clave = await c.digest("SHA-256", new TextEncoder().encode(token));
  const k = await c.importKey("raw", clave, { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  return hex(await c.sign("HMAC", k, new TextEncoder().encode(`${reto}|${tunel}`)));
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
  #ws: WebSocket | null = null;
  #cortadoDesde: number | null = null;
  #final = false;
  #alPrimeraVista: (() => void) | null = null;
  /** Se cumple cuando llega la primera vista (la app puede empezar a leer). */
  readonly lista: Promise<void>;

  constructor(enlace: Enlace) {
    this.enlace = enlace;
    this.lista = new Promise((r) => (this.#alPrimeraVista = r));
  }

  /** Lo que la app pide con `invoke` se contesta con la réplica. */
  responder(cmd: string, args: Record<string, unknown>): Promise<unknown> {
    try {
      return Promise.resolve(responder(cmd, args, this.vista));
    } catch (err) {
      return Promise.reject(err);
    }
  }

  /** URL de una imagen de la partida (la ruta puede llevar la marca de miniatura de `obtener_miniatura`). */
  urlImagen(ref: string): string {
    if (!this.sesion) return "";
    const { ruta, mini } = separarMiniatura(ref);
    return urlImagen(this.enlace, this.sesion, ruta, mini);
  }

  conectar() {
    this.#final = false;
    const ws = new WebSocket(urlWebSocket(this.enlace));
    this.#ws = ws;
    ws.onmessage = (e) => {
      let m: MensajeHost;
      try {
        m = JSON.parse(String(e.data));
      } catch {
        return;
      }
      void this.#recibir(m);
    };
    ws.onclose = () => {
      if (this.#ws !== ws || this.#final) return;
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
        return;
      case "Vista": {
        const primera = this.#alPrimeraVista;
        this.vista = m.vista;
        if (primera) {
          this.#alPrimeraVista = null;
          primera();
        } else {
          // Al reconectar: todo puede haber cambiado
          emitirLocal(EVENTO_PARTIDA_CAMBIADA, { zonas: true, mapas: true, entidades: true, definiciones: true });
        }
        return;
      }
      case "Cambios":
        this.vista = aplicarCambios(this.vista, m.cambios);
        emitirLocal(EVENTO_PARTIDA_CAMBIADA, partesCambiadas(m.cambios));
        return;
      case "Aviso":
        // El tablero ya escucha este evento (los avisos del host: ping y recorrido)
        emitirLocal(EVENTO_AVISO, m.aviso);
        return;
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
  const enlace = leerEnlace(location.hash);
  if (!enlace) return null;
  const c = new Conexion(enlace);
  instalarJugador((cmd, args) => c.responder(cmd, args));
  return c;
})();
