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
// Por dónde habla: si el enlace lleva la huella del host (`f`) y el navegador puede, primero la conexión DIRECTA
// (WebRTC, punto a punto, `webrtc.ts`), poniéndose de acuerdo con el host por relés de Nostr (`senal.ts`) sin que el
// jugador haga nada. Si no se puede o algo falla, entra por el respaldo: el WebSocket de siempre (túnel, dirección propia
// o red local). Si la directa se cae, se sigue por el respaldo para no cortar la partida. En los dos casos es un
// `TransporteCliente`.
//
// Imágenes: van por esta misma conexión (`ImagenesRed`). Los `<img>` de la app piden `<base>/__imagen?ruta=…`,
// que contesta el Service Worker (src/service-worker.ts) preguntándole a esta página. Sin Service Worker (p. ej.
// Firefox en ventana privada) se usa el respaldo HTTP de siempre (`/imagen?sesion=…` del túnel).
import { hmac } from "@noble/hashes/hmac.js";
import { sha256 } from "@noble/hashes/sha2.js";
import { bytesToHex } from "@noble/hashes/utils.js";
import { emitirLocal, EVENTO_PARTIDA_CAMBIADA, instalarJugador } from "$lib/backend";
import { EVENTO_AVISO, type AvisoTablero } from "$lib/avisosTablero";
import { esJugador } from "$lib/modo";
import { claveAlmacen, leerEnlace, urlImagen, urlWebSocket, type Enlace } from "./enlace";
import { ImagenesRed, claveImagen } from "./imagenesRed";
import { porWebSocket, type OyentesTransporte, type TransporteCliente } from "./transporte";
import { pedirRespuesta, pedirRespuestaPorServidor } from "./senal";
import { crearDirecto, soportaDirecto, type Directo } from "./webrtc";
import { VERSION_PROTOCOLO, type Jugador, type MensajeCliente, type MensajeHost, type VistaJugador } from "./protocolo";
import { aplicarCambios, partesCambiadas, PETICIONES, responder, separarMiniatura, vistaVacia } from "./replica";

/** Carpeta de la página (`/OpenRoll` en GitHub Pages, vacía en desarrollo): donde vive el Service Worker. */
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
/** Con la respuesta del host ya usada, lo que se espera a que la conexión directa hable antes de pasar al respaldo. */
const TIEMPO_ABRIR_DIRECTO_MS = 6000;
/** Si el servidor del host no negoció lo directo, los relés se prueban en segundo plano a los pocos instantes de entrar. */
const ESPERA_RELES_MS = 500;
/** Tras un fallo de lo directo, no se vuelve a intentar al entrar durante este tiempo (se entra ya por el respaldo). */
const RECORDAR_FALLO_MS = 30 * 60_000;
/** Tras caerse una directa que había funcionado: cuándo se prueba a volver a ella (en segundo plano, 3 intentos). */
export const ESPERAS_MEJORA_MS = [30_000, 120_000, 300_000];
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

const claveFallo = (sala: string) => `openroll.directo.${sala}`;

/** ¿Falló lo directo hace poco en este navegador con esta sala? (Sin almacenamiento: no se sabe, se intenta.) */
function falloReciente(sala: string): boolean {
  try {
    const v = JSON.parse(localStorage.getItem(claveFallo(sala)) ?? "null");
    return typeof v?.fallo === "number" && Date.now() - v.fallo < RECORDAR_FALLO_MS;
  } catch {
    return false;
  }
}

function anotarFallo(sala: string, hay: boolean) {
  try {
    if (hay) localStorage.setItem(claveFallo(sala), JSON.stringify({ fallo: Date.now() }));
    else localStorage.removeItem(claveFallo(sala));
  } catch {
    // Sin almacenamiento: no se recuerda
  }
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
  /** Por dónde se habla ahora con el host (el WebSocket o la conexión directa). */
  #t: TransporteCliente | null = null;
  /** La conexión directa que se está haciendo o ya hecha. */
  #directo: Directo | null = null;
  /** Si se habla por la conexión directa (la prueba del token se ata a la huella del host y no a una dirección). */
  #porDirecto = false;
  /** El intento de conexión directa en curso (se aborta al pasar al respaldo o al cerrar). */
  #intento: AbortController | null = null;
  /** Se cumple al llegar lo primero del host por la directa (señal de que los canales abrieron). */
  #alPrimerMensaje: ((abrio: boolean) => void) | null = null;
  #huboMensaje = false;
  /** Esta sesión ya habló por la directa (si se cae, se prueba a volver a ella). */
  #huboDirecta = false;
  /** Intentos de volver a lo directo desde la última caída. */
  #mejoras = 0;
  /** El servidor no negoció lo directo y el enlace trae relés: se prueban en segundo plano (una vez) al estar dentro. */
  #mejoraPorReles = false;
  #relojMejora: ReturnType<typeof setTimeout> | null = null;
  #mejorando: AbortController | null = null;
  /** Tras caerse la directa, solo se reconecta por el respaldo. */
  #soloRespaldo = false;
  /** Identifica el transporte actual: el cierre de uno anterior (p. ej. la directa, al pasar al respaldo) no cuenta. */
  #idTransporte: symbol | null = null;
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
          if (this.#t?.abierto()) this.#t.enviarBinario(b);
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

  /** Se cumple cuando la conexión tiene sitio para más (al subir una imagen). */
  async #sitio(): Promise<void> {
    while (this.#t?.abierto() && this.#t.pendiente() > MAX_BUFER_SUBIDA) {
      await new Promise((r) => setTimeout(r, 20));
    }
    if (!this.#t?.abierto()) throw "Sin conexión con la sala";
  }

  /**
   * Lo que la app pide con `invoke`: las lecturas se contestan con la réplica; las acciones de la lista blanca
   * se piden al host, que comprueba que puede hacerlas y las ejecuta en su Rust.
   */
  responder(cmd: string, args: Record<string, unknown>): Promise<unknown> {
    // El foco de un jugador es suyo: no se envía a nadie (ni al host)
    if (cmd === "avisar_tablero" && "Foco" in ((args.aviso as object) ?? {})) return Promise.resolve(null);
    // El arrastre en vivo (protocolo 7): sin respuesta ni espera, y con orden propio. Por la conexión directa va por
    // el canal que no reintenta ni ordena, así una pérdida no frena nada
    if (cmd === "avisar_tablero" && ("Arrastre" in (args.aviso as object) || "ArrastreFin" in (args.aviso as object))) {
      this.#enviar({ tipo: "Aviso", aviso: args.aviso as AvisoTablero });
      return Promise.resolve(null);
    }
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
    if (this.estado.tipo !== "dentro" || !this.#t?.abierto()) {
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

  /** Lo que llega del host, venga por donde venga. */
  #oyentes(): OyentesTransporte {
    const id = Symbol("transporte");
    this.#idTransporte = id;
    return this.#crearOyentes(id);
  }

  /**
   * Oyentes de un transporte que se está probando en segundo plano (la vuelta a lo directo): lo que llega se guarda
   * hasta `activar()` (el momento del cambio); si se cierra antes, solo avisa de que el intento falló.
   */
  #oyentesDiferidos(alHablar: () => void, alFallar: () => void) {
    const id = Symbol("transporte");
    const reales = this.#crearOyentes(id);
    let activo = false;
    const cola: Array<() => void> = [];
    const oyentes: OyentesTransporte = {
      alTexto: (t) => {
        if (activo) return reales.alTexto(t);
        cola.push(() => reales.alTexto(t));
        alHablar();
      },
      alBinario: (b) => {
        if (activo) return reales.alBinario(b);
        cola.push(() => reales.alBinario(b));
      },
      alCerrar: () => {
        if (activo) return reales.alCerrar();
        alFallar();
      }
    };
    const activar = () => {
      this.#idTransporte = id;
      activo = true;
      for (const f of cola.splice(0)) f();
    };
    return { oyentes, activar };
  }

  #crearOyentes(id: symbol): OyentesTransporte {
    return {
      alTexto: (texto) => {
        if (this.#idTransporte !== id) return; // un transporte viejo (p. ej. el WebSocket tras pasar a lo directo) no cuenta
        this.#huboMensaje = true;
        this.#alPrimerMensaje?.(true);
        let m: MensajeHost;
        try {
          m = JSON.parse(texto);
        } catch {
          return;
        }
        if (this.imagenes.alMensaje(m)) return;
        void this.#recibir(m);
      },
      alBinario: (datos) => {
        if (this.#idTransporte === id) this.imagenes.alBinario(datos);
      },
      alCerrar: () => {
        if (this.#idTransporte !== id) return; // ya hay otro transporte: este no cuenta
        // La directa se cerró antes de hablar: el intento falla y `#intentarDirecto` pasa al respaldo
        if (this.#alPrimerMensaje) return this.#alPrimerMensaje(false);
        this.#fallarPendientes("Sin conexión con la sala");
        this.imagenes.cortar("Sin conexión con la sala");
        if (this.#final) return;
        this.#t = null;
        if (this.#porDirecto) {
          // Se cayó la directa: se sigue por el respaldo para no cortar la partida (y se intentará volver a lo directo)
          this.#porDirecto = false;
          this.#soloRespaldo = true;
          this.#directo = null;
          this.#mejoras = 0;
          this.#programarMejora();
        }
        this.#reintentar();
      }
    };
  }

  /** Empieza a conectar: directo si se puede (el enlace lleva la huella y el navegador sabe), si no por el respaldo. */
  conectar() {
    this.#final = false;
    if (this.#soloRespaldo || !this.enlace.huella || !soportaDirecto() || falloReciente(this.enlace.sala)) return this.conectarPorRespaldo();
    void this.#intentarDirecto();
  }

  /** El WebSocket de siempre: túnel, dirección propia o red local. */
  conectarPorRespaldo() {
    this.#final = false;
    this.#soloRespaldo = true;
    this.#intento?.abort();
    this.#intento = null;
    this.#alPrimerMensaje = null;
    // Primero el transporte nuevo (así el cierre del anterior ya no cuenta), luego se suelta el viejo
    const oyentes = this.#oyentes();
    const viejo = this.#directo;
    this.#directo = null;
    this.#porDirecto = false;
    this.estado = { tipo: "conectando" };
    viejo?.transporte.cerrar();
    this.#t = porWebSocket(urlWebSocket(this.enlace), oyentes);
  }

  /**
   * La conexión directa, sola: abre los canales, intercambia los códigos con el host POR SU SERVIDOR (2,5 s como mucho) y
   * espera a que hable. Si el servidor no negocia, entra ya por el respaldo y, en segundo plano, prueba con los relés
   * (si el enlace los trae) para cambiarse solo; si negoció y la directa no se abre, entra por el respaldo y lo recuerda.
   * Nunca se prueban a la vez: así no se cuenta la IP a un tercero cuando no hace falta.
   */
  async #intentarDirecto() {
    const intento = new AbortController();
    this.#intento = intento;
    this.#huboMensaje = false;
    this.estado = { tipo: "conectando" };
    const vale = () => this.#intento === intento;
    let negociado = false;
    try {
      const d = await crearDirecto(this.enlace, this.#oyentes());
      if (!vale()) return d.transporte.cerrar();
      this.#directo = d;
      this.#t = d.transporte;
      const respuesta = await pedirRespuestaPorServidor({
        secreto: this.enlace.secreto,
        url: urlWebSocket(this.enlace),
        codigoOferta: d.codigo,
        cancelar: intento.signal
      });
      if (!vale()) return;
      await d.usarRespuesta(respuesta); // comprueba que el certificado es el del enlace
      negociado = true;
      this.#porDirecto = true;
      const abrio =
        this.#huboMensaje ||
        (await new Promise<boolean>((ok) => {
          this.#alPrimerMensaje = ok;
          setTimeout(() => ok(false), TIEMPO_ABRIR_DIRECTO_MS);
        }));
      if (!vale()) return;
      this.#alPrimerMensaje = null;
      if (!abrio) throw "La conexión directa no se abrió";
      this.#huboDirecta = true;
      anotarFallo(this.enlace.sala, false);
    } catch {
      if (!vale()) return;
      if (negociado) {
        // Se pusieron de acuerdo y aun así no abrió (NAT estricto…): la próxima vez se entra ya por el respaldo
        anotarFallo(this.enlace.sala, true);
      } else if ((this.enlace.reles ?? []).length > 0) {
        this.#mejoraPorReles = true; // el servidor no negoció: los relés, en segundo plano
      }
      this.conectarPorRespaldo();
      if (this.#mejoraPorReles) this.#programarMejora();
    }
  }

  /**
   * Pone de acuerdo con el host la conexión directa EN SEGUNDO PLANO (volver a lo directo tras una caída): primero por su
   * SERVIDOR (el túnel o su dirección, antes del saludo) y, si no responde, por los relés de Nostr (solo con Cloudflare).
   * Devuelve el código de la respuesta.
   */
  async #negociar(d: Directo, cancelar: AbortSignal): Promise<string> {
    try {
      return await pedirRespuestaPorServidor({ secreto: this.enlace.secreto, url: urlWebSocket(this.enlace), codigoOferta: d.codigo, cancelar });
    } catch (e) {
      if (cancelar.aborted) throw e;
    }
    return this.#negociarPorReles(d, cancelar);
  }

  /** Lo mismo, pero solo por los relés de Nostr del enlace (los trae únicamente el de Cloudflare). */
  async #negociarPorReles(d: Directo, cancelar: AbortSignal): Promise<string> {
    const reles = this.enlace.reles ?? [];
    if (reles.length === 0) throw "Sin relés para ponerse de acuerdo";
    return pedirRespuesta({ secreto: this.enlace.secreto, codigoOferta: d.codigo, n: d.n, reles, cancelar });
  }

  #cancelarMejora() {
    if (this.#relojMejora) clearTimeout(this.#relojMejora);
    this.#relojMejora = null;
    this.#mejorando?.abort();
    this.#mejorando = null;
  }

  /**
   * Programa el próximo intento en segundo plano: los relés tras fallar el servidor (una vez, enseguida) o, si esta sesión
   * ya habló por la directa y se cayó, volver a ella (a los 30 s, 2 min y 5 min).
   */
  #programarMejora() {
    if (this.#final || this.#porDirecto) return;
    let espera: number;
    if (this.#mejoraPorReles) espera = ESPERA_RELES_MS;
    else if (this.#huboDirecta && this.#mejoras < ESPERAS_MEJORA_MS.length) espera = ESPERAS_MEJORA_MS[this.#mejoras];
    else return;
    if (this.#relojMejora) clearTimeout(this.#relojMejora);
    this.#relojMejora = setTimeout(() => void this.#mejorarADirecto(), espera);
  }

  /**
   * Con la partida en marcha por el respaldo, prueba la conexión directa EN SEGUNDO PLANO (sin tocar el WebSocket): si
   * habla, se cambia con el camino de una reconexión; si no, el jugador ni se entera.
   */
  async #mejorarADirecto() {
    this.#relojMejora = null;
    if (this.#final || this.#porDirecto || !this.enlace.huella || !soportaDirecto()) return;
    // Aún reconectando (o pidiendo nombre): se espera al siguiente turno sin gastar un intento
    if (this.estado.tipo !== "dentro") return this.#programarMejora();
    // Sin token guardado (p. ej. ventana privada) el cambio pediría el nombre otra vez: no se intenta
    if (!leerToken(this.enlace.sala)) return;
    const porReles = this.#mejoraPorReles; // una sola vez: el servidor ya falló al entrar
    this.#mejoraPorReles = false;
    if (!porReles) this.#mejoras++;
    const intento = new AbortController();
    this.#mejorando = intento;
    const vale = () => this.#mejorando === intento && !this.#final;
    let directo: Directo | null = null;
    let cambiado = false;
    let negociado = false;
    try {
      let habla!: (b: boolean) => void;
      const hablo = new Promise<boolean>((ok) => (habla = ok));
      const dif = this.#oyentesDiferidos(
        () => habla(true),
        () => habla(false)
      );
      directo = await crearDirecto(this.enlace, dif.oyentes);
      if (!vale()) throw "cancelado";
      const respuesta = await (porReles ? this.#negociarPorReles(directo, intento.signal) : this.#negociar(directo, intento.signal));
      if (!vale()) throw "cancelado";
      await directo.usarRespuesta(respuesta); // comprueba el certificado del host
      negociado = true;
      const reloj = setTimeout(() => habla(false), TIEMPO_ABRIR_DIRECTO_MS);
      const abrio = await hablo;
      clearTimeout(reloj);
      if (!vale() || !abrio || this.estado.tipo !== "dentro") throw "no se abrió";
      // El cambio sigue el camino de una reconexión: lo pendiente se corta, se suelta el WebSocket y la directa
      // hace el saludo (reto → prueba atada a la huella → bienvenida → vista)
      this.#fallarPendientes("Cambiando de conexión");
      this.imagenes.cortar("Cambiando de conexión");
      const viejo = this.#t;
      this.#directo = directo;
      this.#t = directo.transporte;
      this.#porDirecto = true;
      this.#huboDirecta = true;
      this.#mejoras = 0;
      cambiado = true;
      this.estado = { tipo: "reconectando" };
      this.#idTransporte = null; // lo que diga el viejo, desde ahora, no cuenta
      viejo?.cerrar();
      dif.activar();
    } catch {
      if (!cambiado) directo?.transporte.cerrar();
      // Se pusieron de acuerdo y no abrió, y nunca ha habido directa: es que aquí no puede ser (se recuerda)
      if (negociado && !cambiado && !this.#huboDirecta && vale()) anotarFallo(this.enlace.sala, true);
      if (vale()) this.#programarMejora();
    } finally {
      if (this.#mejorando === intento) this.#mejorando = null;
    }
  }

  /** Nombre y contraseña, cuando el host los pide. */
  unirse(nombre: string, contrasena: string) {
    this.#enviar({ tipo: "Unirse", nombre: nombre.trim(), contrasena: contrasena || undefined });
  }

  cerrar() {
    this.#final = true;
    this.#intento?.abort();
    this.#intento = null;
    this.#cancelarMejora();
    this.#t?.cerrar();
  }

  #enviar(m: MensajeCliente) {
    if (!this.#t?.abierto()) return;
    const texto = JSON.stringify(m);
    // El arrastre en vivo puede perderse: por la conexión directa va por el canal sin reintentos
    if (m.tipo === "Aviso" && "Arrastre" in m.aviso) this.#t.enviarEfimero(texto);
    else this.#t.enviarTexto(texto);
  }

  #terminar(motivo: string) {
    this.#final = true;
    this.#cancelarMejora();
    this.estado = { tipo: "fuera", motivo };
    void this.imagenes.terminar(motivo);
    this.#t?.cerrar();
  }

  #reintentar() {
    const ahora = Date.now();
    this.#cortadoDesde ??= ahora;
    if (ahora - this.#cortadoDesde > REINTENTAR_DURANTE_MS) {
      this.estado = { tipo: "fuera", motivo: "Se ha perdido la conexión con el host" };
      return;
    }
    this.estado = { tipo: "reconectando" };
    setTimeout(() => this.conectarPorRespaldo(), 2000);
  }

  async #recibir(m: MensajeHost) {
    switch (m.tipo) {
      case "Reto": {
        const token = leerToken(this.enlace.sala);
        // La prueba va atada a por dónde se llegó: por la conexión directa, a la huella del host; si no, a su dirección
        const atada = this.#porDirecto && this.enlace.huella ? `webrtc:${this.enlace.huella}` : this.enlace.tunel;
        const p = token ? await prueba(token, m.reto, atada) : undefined;
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
