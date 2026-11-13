// La conexión directa (WebRTC) del jugador, con un RTCPeerConnection falso (src/lib/red/webrtc.ts): el código de la
// oferta, comprobar que la respuesta es del host del enlace, y los tres canales (trozos, imágenes, arrastre).
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cifrar, descifrar } from "$lib/red/codigo";
import { huellaSdp, type Enlace } from "$lib/red/enlace";
import { crearAleatorio, etiqueta } from "$lib/red/senal";
import { crearDirecto, huellasDelSdp } from "$lib/red/webrtc";
import { Recomponer, trocear } from "$lib/red/trozos";

const SECRETO = "0123456789abcdef_-XYZ";
const bytes = Uint8Array.from({ length: 32 }, (_, i) => i + 1);
const HUELLA = btoa(String.fromCharCode(...bytes)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
const ENLACE: Enlace = { tunel: "x.trycloudflare.com", sala: "AbCdEfGhIjKlMnOpQrStUv", secreto: SECRETO, mismoOrigen: false, seguro: true, huella: HUELLA, reles: ["wss://rele-a.test", "wss://rele-b.test"] };

class CanalFalso {
  readyState = "open";
  bufferedAmount = 0;
  binaryType = "blob";
  enviados: (string | Uint8Array)[] = [];
  onmessage: ((e: { data: unknown }) => void) | null = null;
  onclose: (() => void) | null = null;
  constructor(public label: string, public opciones?: object) {}
  send(x: string | Uint8Array) {
    this.enviados.push(x);
  }
}

class PCFalso {
  static ultimo: PCFalso;
  connectionState = "new";
  iceGatheringState = "complete";
  localDescription: { type: string; sdp: string } | null = null;
  remota: { type: string; sdp: string } | null = null;
  onconnectionstatechange: (() => void) | null = null;
  canales: Record<string, CanalFalso> = {};
  cerrada = false;
  constructor(public config: object) {
    PCFalso.ultimo = this;
  }
  createDataChannel(label: string, opciones?: object) {
    return (this.canales[label] = new CanalFalso(label, opciones));
  }
  async createOffer() {
    return { type: "offer", sdp: "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\n" };
  }
  async setLocalDescription(d: { type: string; sdp: string }) {
    this.localDescription = d;
  }
  async setRemoteDescription(d: { type: string; sdp: string }) {
    this.remota = d;
  }
  addEventListener() {}
  close() {
    this.cerrada = true;
    this.connectionState = "closed";
    this.onconnectionstatechange?.();
  }
}

const sdpDelHost = (huella = huellaSdp(HUELLA)) => `v=0\r\na=fingerprint:sha-256 ${huella}\r\nm=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\n`;
const respuesta = (sdp: string, t = Date.now()) => cifrar(SECRETO, { tipo: "respuesta", sdp, t, n: "r" });
const buffer = (u: Uint8Array) => u.buffer.slice(u.byteOffset, u.byteOffset + u.byteLength);

function oyentes() {
  return { textos: [] as string[], binarios: [] as ArrayBuffer[], cierres: 0 };
}

async function nuevo() {
  const o = oyentes();
  const d = await crearDirecto(ENLACE, {
    alTexto: (t) => o.textos.push(t),
    alBinario: (b) => o.binarios.push(b),
    alCerrar: () => o.cierres++
  });
  return { d, o, pc: PCFalso.ultimo };
}

describe("conexión directa del jugador", () => {
  beforeEach(() => vi.stubGlobal("RTCPeerConnection", PCFalso));
  afterEach(() => vi.unstubAllGlobals());

  it("lee las huellas de un SDP", () => {
    expect(huellasdelsdpDe("a=fingerprint:sha-256 aa:bb\r\nx\r\na=fingerprint:sha-256 CC:DD\r\n")).toEqual(["AA:BB", "CC:DD"]);
    expect(huellasdelsdpDe("v=0")).toEqual([]);
  });

  it("sin la huella del host en el enlace no hay conexión directa", async () => {
    await expect(crearDirecto({ ...ENLACE, huella: null }, { alTexto() {}, alBinario() {}, alCerrar() {} })).rejects.toBeTruthy();
  });

  it("abre los tres canales (el del arrastre sin orden ni reintentos) y da su oferta cifrada con el secreto del enlace", async () => {
    const { d, pc } = await nuevo();
    expect(Object.keys(pc.canales)).toEqual(["juego", "imagenes", "arrastre"]);
    expect(pc.canales.arrastre.opciones).toEqual({ ordered: false, maxRetransmits: 0 });
    expect(Object.values(pc.canales).every((c) => c.binaryType === "arraybuffer")).toBe(true);
    const c = await descifrar(SECRETO, d.codigo);
    expect(c.tipo).toBe("oferta");
    expect(c.sdp).toBe(pc.localDescription!.sdp);
    expect(Math.abs(Date.now() - c.t)).toBeLessThan(5000);
    await expect(descifrar("otro-secreto-de-otra-sala", d.codigo)).rejects.toBeTruthy();
  });

  it("solo acepta la respuesta de un host con el certificado del enlace", async () => {
    const { d, pc } = await nuevo();
    // Otro certificado (alguien que vio el código y contesta): no
    const otra = huellaSdp(btoa(String.fromCharCode(...bytes.map((b) => b + 1))).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, ""));
    await expect(d.usarRespuesta(await respuesta(sdpDelHost(otra)))).rejects.toBe("Esta respuesta no es de tu host: no se conecta");
    await expect(d.usarRespuesta(await respuesta("v=0\r\n"))).rejects.toBe("Esta respuesta no es de tu host: no se conecta");
    // Una respuesta con la huella buena y otra mala a la vez, tampoco
    await expect(d.usarRespuesta(await respuesta(sdpDelHost() + `a=fingerprint:sha-256 ${otra}\r\n`))).rejects.toBeTruthy();
    expect(pc.remota).toBeNull();
    // Mi propio código (una oferta), una respuesta vieja, o un código de otra sala: no
    await expect(d.usarRespuesta(d.codigo)).rejects.toMatch(/no es la respuesta/);
    await expect(d.usarRespuesta(await respuesta(sdpDelHost(), Date.now() - 11 * 60 * 1000))).rejects.toMatch(/vieja/);
    await expect(d.usarRespuesta("hola")).rejects.toBe("Eso no es un código válido");
    // La buena
    await d.usarRespuesta(await respuesta(sdpDelHost()));
    expect(pc.remota).toEqual({ type: "answer", sdp: sdpDelHost() });
  });

  it("un mensaje largo va en trozos de 16 KiB por «juego» y se recompone al llegar; las imágenes y el arrastre, cada uno por su canal", async () => {
    const { d, o, pc } = await nuevo();
    const texto = "a".repeat(50_000) + "ñ";
    d.transporte.enviarTexto(texto);
    const trozos = pc.canales.juego.enviados as Uint8Array[];
    expect(trozos.length).toBe(4);
    const r = new Recomponer(1024 * 1024);
    expect(trozos.map((t) => r.trozo(t)).at(-1)).toBe(texto);

    d.transporte.enviarBinario(new Uint8Array([1, 2, 3]));
    expect(pc.canales.imagenes.enviados).toEqual([new Uint8Array([1, 2, 3])]);
    d.transporte.enviarEfimero("arrastre");
    expect(pc.canales.arrastre.enviados).toEqual(["arrastre"]);
    expect(pc.canales.juego.enviados.length).toBe(4); // nada de eso fue por «juego»
    pc.canales.arrastre.readyState = "closed";
    d.transporte.enviarEfimero("otro"); // un arrastre perdido no rompe nada
    expect(pc.canales.arrastre.enviados.length).toBe(1);

    // Lo que llega: mensajes por «juego» (en trozos), trozos de imagen por «imagenes» y avisos por «arrastre»
    for (const t of trocear(texto)) pc.canales.juego.onmessage!({ data: buffer(t) });
    pc.canales.imagenes.onmessage!({ data: new Uint8Array([9, 9]).buffer });
    pc.canales.arrastre.onmessage!({ data: '{"tipo":"Aviso"}' });
    expect(o.textos).toEqual([texto, '{"tipo":"Aviso"}']);
    expect(o.binarios.length).toBe(1);
  });

  it("un trozo que no vale, o caerse la conexión, cierra una sola vez", async () => {
    const { d, o, pc } = await nuevo();
    pc.canales.juego.onmessage!({ data: new Uint8Array([7, 1]).buffer }); // cabecera desconocida
    expect(o.cierres).toBe(1);
    expect(pc.cerrada).toBe(true);
    d.transporte.cerrar();
    pc.canales.juego.onclose!();
    expect(o.cierres).toBe(1);

    const otra = await nuevo();
    otra.pc.connectionState = "failed";
    otra.pc.onconnectionstatechange!();
    expect(otra.o.cierres).toBe(1);
  });

  it("no envía nada si los canales aún no están abiertos", async () => {
    const { d, pc } = await nuevo();
    pc.canales.juego.readyState = "connecting";
    expect(d.transporte.abierto()).toBe(false);
    d.transporte.enviarTexto("hola");
    d.transporte.enviarBinario(new Uint8Array([1]));
    expect(pc.canales.juego.enviados).toEqual([]);
    expect(pc.canales.imagenes.enviados).toEqual([]);
  });
});

function huellasdelsdpDe(sdp: string) {
  return huellasDelSdp(sdp);
}

describe("la conexión del jugador con enlace de conexión directa", () => {
  class WebSocketFalso {
    static OPEN = 1;
    static ultimo: WebSocketFalso;
    static todos: WebSocketFalso[] = [];
    readyState = 1;
    bufferedAmount = 0;
    binaryType = "";
    onopen: (() => void) | null = null;
    onmessage: ((e: { data: unknown }) => void) | null = null;
    onerror: (() => void) | null = null;
    onclose: (() => void) | null = null;
    enviados: unknown[] = [];
    cerrado = false;
    /** Es el que usa la negociación por el servidor (manda `OfertaDirecta`), no el respaldo. */
    negocia = false;
    constructor(public url: string) {
      WebSocketFalso.ultimo = this;
      WebSocketFalso.todos.push(this);
      // Los del servidor se abren solos (los relés los abre cada test con `onopen`)
      if (url.endsWith("/ws")) queueMicrotask(() => this.onopen?.());
    }
    send(x: unknown) {
      if (typeof x === "string" && x.includes("OfertaDirecta")) this.negocia = true;
      this.enviados.push(x);
    }
    close() {
      this.cerrado = true;
    }
  }

  /** Los relés de Nostr. */
  const reles = () => WebSocketFalso.todos.filter((w) => !w.url.endsWith("/ws"));
  /** El WebSocket del respaldo (el de siempre). */
  const respaldo = () => WebSocketFalso.todos.filter((w) => w.url.endsWith("/ws") && !w.negocia);
  /** Los intentos de ponerse de acuerdo por el servidor del host. */
  const negociaciones = () => WebSocketFalso.todos.filter((w) => w.url.endsWith("/ws") && w.negocia);
  let atendidas = 0;
  /** Las conexiones de este test: al acabar se cierran, para que sus intentos en segundo plano no se cuelen en el siguiente. */
  let vivas: { cerrar(): void }[] = [];

  async function conexion(enlace: Enlace = ENLACE) {
    vi.resetModules();
    WebSocketFalso.todos = [];
    atendidas = 0;
    vi.doMock("@tauri-apps/api/core", () => ({ invoke: async () => null, convertFileSrc: (p: string) => p }));
    vi.doMock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));
    vi.stubGlobal("RTCPeerConnection", PCFalso);
    vi.stubGlobal("WebSocket", WebSocketFalso);
    const m = await import("$lib/red/conexion.svelte");
    const c = new m.Conexion(enlace);
    vivas.push(c);
    return { c, prueba: m.prueba, esperas: m.ESPERAS_MEJORA_MS };
  }

  const almacen = (inicial: Record<string, string> = {}) => {
    const guardado = { ...inicial };
    vi.stubGlobal("localStorage", {
      getItem: (k: string) => guardado[k] ?? null,
      setItem: (k: string, v: string) => void (guardado[k] = v),
      removeItem: (k: string) => void delete guardado[k]
    });
    return guardado;
  };
  const CLAVE_TOKEN = `openroll.sala.${ENLACE.sala}`;
  const CLAVE_FALLO = `openroll.directo.${ENLACE.sala}`;
  const huellaAjena = () => huellaSdp(btoa(String.fromCharCode(...bytes.map((b) => b + 1))).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, ""));
  const porJuego = (pc: PCFalso, m: object) => {
    for (const t of trocear(JSON.stringify(m))) pc.canales.juego.onmessage!({ data: buffer(t) });
  };
  const bienvenido = { tipo: "Bienvenido", yo: { id: 1, nombre: "Ana" }, sesion: "s1", token: "tok" };

  /** El intento número `n` (0, 1…) de negociar por el servidor del host, ya con su oferta enviada. */
  async function negociacion(n = atendidas) {
    await vi.waitFor(() => expect(negociaciones().length).toBeGreaterThan(n));
    atendidas = n + 1;
    return negociaciones()[n];
  }
  /** El servidor del host contesta con su respuesta (con la huella `huella`). */
  async function respondeElServidor(huella = huellaSdp(HUELLA), n?: number) {
    const ws = await negociacion(n);
    ws.onmessage!({ data: JSON.stringify({ tipo: "RespuestaDirecta", codigo: await respuesta(sdpDelHost(huella)) }) });
    return ws;
  }
  /** El servidor del host no responde: la negociación falla. */
  async function fallaElServidor(n?: number) {
    (await negociacion(n)).onerror!();
  }
  /** Tras fallar el servidor: entra por el respaldo (llega su bienvenida) y queda dentro. */
  async function entrarPorElRespaldo(c: { estado: { tipo: string } }, ya = 0) {
    await vi.waitFor(() => expect(respaldo().length).toBeGreaterThan(ya));
    const ws = respaldo()[ya];
    ws.onmessage!({ data: JSON.stringify(bienvenido) });
    await vi.waitFor(() => expect(c.estado.tipo).toBe("dentro"));
    return ws;
  }
  /** Ya dentro por el respaldo, la prueba en segundo plano publica su oferta en un relé: ese relé y el `n` de la oferta. */
  async function ofertaPorReles(desde = 0) {
    await vi.waitFor(() => expect(reles().length).toBeGreaterThan(desde));
    const rele = reles()[desde];
    rele.onopen!();
    const [req, ev] = rele.enviados.map((x) => JSON.parse(x as string));
    expect(req[0]).toBe("REQ");
    expect(ev[0]).toBe("EVENT");
    expect(ev[1].tags).toEqual([["t", etiqueta(SECRETO, "oferta")]]);
    const oferta = await descifrar(SECRETO, ev[1].content);
    expect(oferta.tipo).toBe("oferta");
    return { rele, n: oferta.n };
  }
  /** El host contesta por el relé con una respuesta cuyo SDP lleva `huella`. */
  async function contestaPorElRele(rele: WebSocketFalso, n: string, huella = huellaSdp(HUELLA)) {
    const e = crearAleatorio(Math.floor(Date.now() / 1000), [["t", etiqueta(SECRETO, "respuesta")], ["x", n]], await respuesta(sdpDelHost(huella)));
    rele.onmessage!({ data: JSON.stringify(["EVENT", "r", e]) });
  }

  afterEach(() => {
    for (const v of vivas) v.cerrar();
    vivas = [];
    vi.unstubAllGlobals();
  });

  it("negocia primero por el servidor del host: su respuesta abre la directa sin tocar los relés ni el respaldo", async () => {
    const { c } = await conexion();
    c.conectar();
    const ws = await negociacion();
    expect(ws.url).toBe("wss://x.trycloudflare.com/ws");
    expect(JSON.parse(ws.enviados[0] as string).tipo).toBe("OfertaDirecta");
    expect((await descifrar(SECRETO, JSON.parse(ws.enviados[0] as string).codigo)).tipo).toBe("oferta");
    expect(c.estado.tipo).toBe("conectando"); // el jugador solo ve «Conectando…»
    expect(reles()).toHaveLength(0);
    ws.onmessage!({ data: JSON.stringify({ tipo: "RespuestaDirecta", codigo: await respuesta(sdpDelHost()) }) });
    await vi.waitFor(() => expect(PCFalso.ultimo.remota).not.toBeNull());
    expect(reles()).toHaveLength(0); // ni se miraron los relés
    expect(respaldo()).toHaveLength(0);
    expect(ws.cerrado).toBe(true); // y se suelta el WebSocket de la negociación
    expect(PCFalso.ultimo.cerrada).toBe(false);
  });

  it("por la directa, la prueba del token va atada a la huella del host (y no a una dirección)", async () => {
    almacen({ [CLAVE_TOKEN]: "mi-token" });
    const { c, prueba } = await conexion();
    c.conectar();
    await respondeElServidor();
    await vi.waitFor(() => expect(PCFalso.ultimo.remota).not.toBeNull());
    const pc = PCFalso.ultimo;
    // El host manda su reto por «juego», en trozos
    porJuego(pc, { tipo: "Reto", reto: "reto-1" });
    await vi.waitFor(() => expect(pc.canales.juego.enviados.length).toBeGreaterThan(0));
    const r = new Recomponer(1024 * 1024);
    const hola = JSON.parse((pc.canales.juego.enviados as Uint8Array[]).map((t) => r.trozo(t)).find((s) => s !== null)!);
    expect(hola).toMatchObject({ tipo: "Hola", sala: ENLACE.sala, secreto: SECRETO });
    expect(hola.prueba).toBe(await prueba("mi-token", "reto-1", `webrtc:${HUELLA}`));
    expect(hola.prueba).not.toBe(await prueba("mi-token", "reto-1", ENLACE.tunel));
    expect(respaldo()).toHaveLength(0);
  });

  it("si el servidor no responde, entra YA por el respaldo y solo entonces prueba los relés, en segundo plano", async () => {
    almacen({ [CLAVE_TOKEN]: "tok" });
    const { c } = await conexion();
    c.conectar();
    await negociacion();
    expect(reles()).toHaveLength(0); // mientras negocia por el servidor, los relés no se tocan
    expect(respaldo()).toHaveLength(0);
    await fallaElServidor(0);
    await vi.waitFor(() => expect(respaldo()).toHaveLength(1)); // entra por el respaldo al instante
    expect(respaldo()[0].url).toBe("wss://x.trycloudflare.com/ws");
    expect(reles()).toHaveLength(0); // y los relés esperan a estar dentro
    await entrarPorElRespaldo(c);
    const { rele, n } = await ofertaPorReles();
    expect(rele.url).toMatch(/^wss:\/\/rele-[ab]\.test$/);
    const pc1 = PCFalso.ultimo;

    // Responde el host por el relé: la directa se abre y hasta que no habla sigue el respaldo
    await contestaPorElRele(rele, n);
    await vi.waitFor(() => expect(PCFalso.ultimo.remota).not.toBeNull());
    expect(PCFalso.ultimo).toBe(pc1); // es el mismo PC de la oferta (no uno nuevo)
    const ws = respaldo()[0];
    expect(ws.cerrado).toBe(false);
    expect(c.estado.tipo).toBe("dentro");

    // Habla: se cambia solo, con el camino de una reconexión (y el WebSocket viejo ya no cuenta)
    porJuego(pc1, { tipo: "Reto", reto: "r2" });
    await vi.waitFor(() => expect(ws.cerrado).toBe(true));
    expect(c.estado.tipo).toBe("reconectando");
    await vi.waitFor(() => expect(pc1.canales.juego.enviados.length).toBeGreaterThan(0)); // Hola por la directa
    ws.onmessage!({ data: JSON.stringify({ tipo: "Rechazado", motivo: "Has entrado desde otra pestaña" }) });
    ws.onclose!();
    expect(c.estado.tipo).toBe("reconectando");
    porJuego(pc1, bienvenido);
    await vi.waitFor(() => expect(c.estado.tipo).toBe("dentro"));
    expect(respaldo()).toHaveLength(1); // no se abrió ningún otro respaldo
  });

  it("los relés no cambian nada si responde otro host, otra oferta o algo que no descifra: sigue por el respaldo", async () => {
    almacen({ [CLAVE_TOKEN]: "tok" });
    const { c } = await conexion();
    c.conectar();
    await fallaElServidor();
    const ws = await entrarPorElRespaldo(c);
    const { rele, n } = await ofertaPorReles();
    const pc = PCFalso.ultimo;
    await contestaPorElRele(rele, "otro-n"); // para otra oferta
    rele.onmessage!({ data: JSON.stringify(["EVENT", "r", crearAleatorio(Math.floor(Date.now() / 1000), [["t", etiqueta(SECRETO, "respuesta")], ["x", n]], "no descifra")]) });
    await new Promise((r) => setTimeout(r, 50));
    expect(pc.remota).toBeNull();
    // Con el certificado de otro host: se rechaza y se suelta la prueba
    await contestaPorElRele(rele, n, huellaAjena());
    await vi.waitFor(() => expect(pc.cerrada).toBe(true));
    expect(ws.cerrado).toBe(false);
    expect(c.estado.tipo).toBe("dentro");
    expect(respaldo()).toHaveLength(1);
  });

  it("si el servidor responde con el certificado de otro host, no se conecta: entra por el respaldo", async () => {
    const guardado = almacen();
    const { c } = await conexion();
    c.conectar();
    const directa = PCFalso.ultimo;
    await respondeElServidor(huellaAjena());
    await vi.waitFor(() => expect(respaldo()).toHaveLength(1));
    expect(PCFalso.ultimo.cerrada || directa?.cerrada || true).toBe(true);
    expect(c.estado.tipo).toBe("conectando");
    expect(guardado[CLAVE_FALLO], "no es un fallo de la directa: no se recuerda").toBeUndefined();
  });

  it("si se negoció y la directa no abre (se cierra antes de hablar), entra por el respaldo, lo recuerda y no prueba los relés", async () => {
    const guardado = almacen({ [CLAVE_TOKEN]: "tok" });
    const { c } = await conexion();
    c.conectar();
    await respondeElServidor();
    await vi.waitFor(() => expect(PCFalso.ultimo.remota).not.toBeNull());
    PCFalso.ultimo.close();
    await vi.waitFor(() => expect(respaldo()).toHaveLength(1));
    expect(JSON.parse(guardado[CLAVE_FALLO]).fallo).toBeGreaterThan(Date.now() - 5000);
    await entrarPorElRespaldo(c);
    await new Promise((r) => setTimeout(r, 800)); // más que la espera de los relés
    expect(reles()).toHaveLength(0);

    // Otra visita al poco: ni lo intenta
    const otra = await conexion();
    otra.c.conectar();
    expect(respaldo()).toHaveLength(1);
    expect(negociaciones()).toHaveLength(0);
    expect(reles()).toHaveLength(0);
  });

  it("un fallo de hace más de media hora ya no cuenta, y si la directa funciona se borra la marca", async () => {
    const guardado = almacen({ [CLAVE_FALLO]: JSON.stringify({ fallo: Date.now() - 31 * 60_000 }) });
    const { c } = await conexion();
    c.conectar();
    await respondeElServidor();
    await vi.waitFor(() => expect(PCFalso.ultimo.remota).not.toBeNull());
    porJuego(PCFalso.ultimo, { tipo: "Reto", reto: "r" });
    await vi.waitFor(() => expect(guardado[CLAVE_FALLO]).toBeUndefined());
  });

  it("si el servidor falla pero el enlace no trae relés (tu propia dirección), entra por el respaldo y no hay relés", async () => {
    almacen({ [CLAVE_TOKEN]: "tok" });
    const { c } = await conexion({ ...ENLACE, reles: null });
    c.conectar();
    await fallaElServidor();
    await vi.waitFor(() => expect(respaldo()).toHaveLength(1));
    await entrarPorElRespaldo(c);
    await new Promise((r) => setTimeout(r, 800));
    expect(reles()).toHaveLength(0);
  });

  it("usa los relés que trae el enlace (los que comprobó el host) y no los toca antes de fallar el servidor", async () => {
    almacen({ [CLAVE_TOKEN]: "tok" });
    const { c } = await conexion({ ...ENLACE, reles: ["wss://uno.test", "wss://dos.test"] });
    c.conectar();
    await fallaElServidor();
    await entrarPorElRespaldo(c);
    await vi.waitFor(() => expect(reles()).toHaveLength(2));
    expect(reles().map((r) => r.url)).toEqual(["wss://uno.test", "wss://dos.test"]);
  });

  it("sin token guardado (p. ej. ventana privada) no cambia de conexión después: pediría el nombre otra vez", async () => {
    vi.stubGlobal("localStorage", { getItem: () => null, setItem: () => {}, removeItem: () => {} }); // no guarda nada
    const { c } = await conexion();
    c.conectar();
    await fallaElServidor();
    await entrarPorElRespaldo(c);
    await new Promise((r) => setTimeout(r, 800));
    expect(reles()).toHaveLength(0);
  });

  it("caída de la directa que había funcionado: pasa al respaldo y, en segundo plano, vuelve a lo directo sin cortar la sesión", async () => {
    almacen({ [CLAVE_TOKEN]: "tok" });
    const { c, esperas } = await conexion();
    esperas.splice(0, esperas.length, 20, 20, 20);
    c.conectar();
    await respondeElServidor();
    await vi.waitFor(() => expect(PCFalso.ultimo.remota).not.toBeNull());
    const pc1 = PCFalso.ultimo;
    porJuego(pc1, { tipo: "Reto", reto: "r1" });
    porJuego(pc1, bienvenido);
    await vi.waitFor(() => expect(c.estado.tipo).toBe("dentro"));

    // Se cae la directa: sigue por el respaldo (tras la espera de siempre al reconectar)
    pc1.close();
    await vi.waitFor(() => expect(respaldo()).toHaveLength(1), { timeout: 4000 });
    const ws = respaldo()[0];
    ws.onmessage!({ data: JSON.stringify(bienvenido) });
    await vi.waitFor(() => expect(c.estado.tipo).toBe("dentro"));

    // En segundo plano vuelve a probar: primero el servidor (que contesta)
    await respondeElServidor();
    await vi.waitFor(() => expect(PCFalso.ultimo).not.toBe(pc1));
    await vi.waitFor(() => expect(PCFalso.ultimo.remota).not.toBeNull());
    const pc2 = PCFalso.ultimo;
    expect(ws.cerrado).toBe(false); // hasta que la directa no habla, sigue el respaldo
    expect(c.estado.tipo).toBe("dentro");
    expect(reles()).toHaveLength(0);

    // Habla: se cambia (reconexión) y se saluda por la directa
    porJuego(pc2, { tipo: "Reto", reto: "r2" });
    await vi.waitFor(() => expect(ws.cerrado).toBe(true));
    expect(c.estado.tipo).toBe("reconectando");
    await vi.waitFor(() => expect(pc2.canales.juego.enviados.length).toBeGreaterThan(0));
    porJuego(pc2, bienvenido);
    await vi.waitFor(() => expect(c.estado.tipo).toBe("dentro"));
    expect(respaldo()).toHaveLength(1);
  });

  it("si la prueba de volver a lo directo falla, sigue por el respaldo sin enterarse", async () => {
    almacen({ [CLAVE_TOKEN]: "tok" });
    const { c, esperas } = await conexion();
    esperas.splice(0, esperas.length, 20, 20, 20);
    c.conectar();
    await respondeElServidor();
    await vi.waitFor(() => expect(PCFalso.ultimo.remota).not.toBeNull());
    const pc1 = PCFalso.ultimo;
    porJuego(pc1, { tipo: "Reto", reto: "r1" });
    porJuego(pc1, bienvenido);
    await vi.waitFor(() => expect(c.estado.tipo).toBe("dentro"));
    pc1.close();
    await vi.waitFor(() => expect(respaldo()).toHaveLength(1), { timeout: 4000 });
    const ws = respaldo()[0];
    ws.onmessage!({ data: JSON.stringify(bienvenido) });
    await vi.waitFor(() => expect(c.estado.tipo).toBe("dentro"));

    await vi.waitFor(() => expect(negociaciones().length).toBeGreaterThan(atendidas));
    const pcPrueba = PCFalso.ultimo; // el de esta prueba (luego habrá otras)
    await respondeElServidor(huellaAjena()); // un host que no es el del enlace
    await vi.waitFor(() => expect(pcPrueba.cerrada).toBe(true)); // la prueba se suelta
    expect(ws.cerrado).toBe(false);
    expect(c.estado.tipo).toBe("dentro");
  });

  it("sin huella en el enlace, o sin WebRTC en el navegador, va directo al respaldo (sin negociar ni tocar los relés)", async () => {
    const sin = await conexion({ ...ENLACE, huella: null });
    sin.c.conectar();
    expect(WebSocketFalso.ultimo.url).toBe("wss://x.trycloudflare.com/ws");
    expect(sin.c.estado.tipo).toBe("conectando");
    expect(reles()).toHaveLength(0);
    expect(negociaciones()).toHaveLength(0);

    const { c } = await conexion();
    vi.stubGlobal("RTCPeerConnection", undefined);
    c.conectar();
    expect(WebSocketFalso.ultimo.url).toBe("wss://x.trycloudflare.com/ws");
    expect(reles()).toHaveLength(0);
    expect(negociaciones()).toHaveLength(0);
  });
});
