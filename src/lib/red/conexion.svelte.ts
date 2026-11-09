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
import { aplicarCambios, partesCambiadas, PETICIONES, responder, separarMiniatura, vistaVacia } from "./replica";

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
  /** Los comandos de dados que ha escrito este jugador (flechas ↑ ↓ del chat). Solo en esta pestaña. */
  #personales: string[] = [];
  /** Acciones enviadas al host que aún no han tenido respuesta. */
  #pendientes = new Map<number, { ok: (v: unknown) => void; error: (e: unknown) => void; reloj: ReturnType<typeof setTimeout> }>();
  #siguiente = 1;
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

  /** Sube la foto de una de sus fichas al host. Devuelve la ruta ("imagenes/…") para ponerla en la ficha. */
  async subirImagen(archivo: File): Promise<string> {
    if (!this.sesion) throw "Sin conexión con la sala";
    const ext = /\.([A-Za-z0-9]+)$/.exec(archivo.name)?.[1] ?? "";
    const q = new URLSearchParams({ sesion: this.sesion, ext });
    const r = await fetch(`https://${this.enlace.tunel}/subir?${q}`, { method: "POST", body: archivo });
    if (!r.ok) throw (await r.text().catch(() => "")) || `No se pudo subir la imagen (${r.status})`;
    return String((await r.json()).ruta);
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
      if (this.#ws !== ws) return;
      this.#fallarPendientes("Sin conexión con la sala");
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
  const enlace = leerEnlace(location.hash);
  if (!enlace) return null;
  const c = new Conexion(enlace);
  instalarJugador((cmd, args) => c.responder(cmd, args));
  return c;
})();
