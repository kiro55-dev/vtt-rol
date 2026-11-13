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
const ENLACE: Enlace = { tunel: "x.trycloudflare.com", sala: "AbCdEfGhIjKlMnOpQrStUv", secreto: SECRETO, mismoOrigen: false, seguro: true, huella: HUELLA };

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
    constructor(public url: string) {
      WebSocketFalso.ultimo = this;
      WebSocketFalso.todos.push(this);
    }
    send(x: unknown) {
      this.enviados.push(x);
    }
    cerrado = false;
    close() {
      this.cerrado = true;
    }
  }

  /** Los relés de Nostr (el respaldo es el único que termina en `/ws`). */
  const reles = () => WebSocketFalso.todos.filter((w) => !w.url.endsWith("/ws"));
  const respaldo = () => WebSocketFalso.todos.filter((w) => w.url.endsWith("/ws"));

  async function conexion(enlace: Enlace = ENLACE) {
    vi.resetModules();
    WebSocketFalso.todos = [];
    vi.doMock("@tauri-apps/api/core", () => ({ invoke: async () => null, convertFileSrc: (p: string) => p }));
    vi.doMock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));
    vi.stubGlobal("RTCPeerConnection", PCFalso);
    vi.stubGlobal("WebSocket", WebSocketFalso);
    const m = await import("$lib/red/conexion.svelte");
    return { c: new m.Conexion(enlace), prueba: m.prueba, esperas: m.ESPERAS_MEJORA_MS };
  }

  /** El jugador publica su oferta en un relé; devuelve ese relé y el `n` de la oferta. */
  async function ofertaPublicada() {
    await vi.waitFor(() => expect(reles().length).toBeGreaterThan(0));
    const rele = reles()[0];
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
  async function contestaElHost(rele: WebSocketFalso, n: string, huella = huellaSdp(HUELLA)) {
    const e = crearAleatorio(Math.floor(Date.now() / 1000), [["t", etiqueta(SECRETO, "respuesta")], ["x", n]], await respuesta(sdpDelHost(huella)));
    rele.onmessage!({ data: JSON.stringify(["EVENT", "r", e]) });
  }

  afterEach(() => vi.unstubAllGlobals());

  it("la oferta sale por los relés, la respuesta del host abre la directa y no hace falta tocar nada", async () => {
    const { c } = await conexion();
    c.conectar();
    const { rele, n } = await ofertaPublicada();
    expect(c.estado.tipo).toBe("conectando"); // el jugador solo ve «Conectando…»
    await contestaElHost(rele, n);
    await vi.waitFor(() => expect(PCFalso.ultimo.remota).not.toBeNull());
    expect(respaldo()).toHaveLength(0); // sigue por la directa: no se abrió el respaldo
    expect(PCFalso.ultimo.cerrada).toBe(false);
  });

  it("una respuesta con el certificado de otro host se rechaza y se entra solo por el respaldo", async () => {
    const { c } = await conexion();
    c.conectar();
    const { rele, n } = await ofertaPublicada();
    const otra = huellaSdp(btoa(String.fromCharCode(...bytes.map((b) => b + 1))).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, ""));
    const directa = PCFalso.ultimo;
    await contestaElHost(rele, n, otra);
    await vi.waitFor(() => expect(respaldo()).toHaveLength(1));
    expect(respaldo()[0].url).toBe("wss://x.trycloudflare.com/ws");
    expect(directa.cerrada).toBe(true); // se suelta la directa
    expect(c.estado.tipo).toBe("conectando"); // y su cierre no dispara una reconexión
  });

  it("una respuesta para otra oferta, o que no descifra, se ignora", async () => {
    const { c } = await conexion();
    c.conectar();
    const { rele, n } = await ofertaPublicada();
    await contestaElHost(rele, "otro-n");
    const falsa = crearAleatorio(Math.floor(Date.now() / 1000), [["t", etiqueta(SECRETO, "respuesta")], ["x", n]], "no descifra");
    rele.onmessage!({ data: JSON.stringify(["EVENT", "r", falsa]) });
    await new Promise((r) => setTimeout(r, 50));
    expect(PCFalso.ultimo.remota).toBeNull();
    expect(respaldo()).toHaveLength(0);
  });

  it("si la directa se cierra antes de hablar, entra por el respaldo", async () => {
    const { c } = await conexion();
    c.conectar();
    const { rele, n } = await ofertaPublicada();
    await contestaElHost(rele, n);
    await vi.waitFor(() => expect(PCFalso.ultimo.remota).not.toBeNull());
    PCFalso.ultimo.close();
    await vi.waitFor(() => expect(respaldo()).toHaveLength(1));
    expect(respaldo()[0].url).toBe("wss://x.trycloudflare.com/ws");
  });

  it("por la directa, la prueba del token va atada a la huella del host (y no a una dirección)", async () => {
    const guardado: Record<string, string> = { [`openroll.sala.${ENLACE.sala}`]: "mi-token" };
    vi.stubGlobal("localStorage", { getItem: (k: string) => guardado[k] ?? null, setItem: () => {} });
    const { c, prueba } = await conexion();
    c.conectar();
    const { rele, n } = await ofertaPublicada();
    await contestaElHost(rele, n);
    await vi.waitFor(() => expect(PCFalso.ultimo.remota).not.toBeNull());
    const pc = PCFalso.ultimo;
    // El host manda su reto por «juego», en trozos
    for (const t of trocear(JSON.stringify({ tipo: "Reto", reto: "reto-1" }))) pc.canales.juego.onmessage!({ data: buffer(t) });
    await vi.waitFor(() => expect(pc.canales.juego.enviados.length).toBeGreaterThan(0));
    const r = new Recomponer(1024 * 1024);
    const hola = JSON.parse((pc.canales.juego.enviados as Uint8Array[]).map((t) => r.trozo(t)).find((s) => s !== null)!);
    expect(hola).toMatchObject({ tipo: "Hola", sala: ENLACE.sala, secreto: SECRETO });
    expect(hola.prueba).toBe(await prueba("mi-token", "reto-1", `webrtc:${HUELLA}`));
    expect(hola.prueba).not.toBe(await prueba("mi-token", "reto-1", ENLACE.tunel));
    expect(respaldo()).toHaveLength(0);
  });

  const almacen = (inicial: Record<string, string> = {}) => {
    const guardado = { ...inicial };
    vi.stubGlobal("localStorage", {
      getItem: (k: string) => guardado[k] ?? null,
      setItem: (k: string, v: string) => void (guardado[k] = v),
      removeItem: (k: string) => void delete guardado[k]
    });
    return guardado;
  };
  const huellaAjena = () => huellaSdp(btoa(String.fromCharCode(...bytes.map((b) => b + 1))).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, ""));
  const porJuego = (pc: PCFalso, m: object) => {
    for (const t of trocear(JSON.stringify(m))) pc.canales.juego.onmessage!({ data: buffer(t) });
  };
  const bienvenido = { tipo: "Bienvenido", yo: { id: 1, nombre: "Ana" }, sesion: "s1", token: "tok" };

  it("si lo directo falla se recuerda y la próxima vez se entra ya por el respaldo; si funciona, se olvida", async () => {
    const clave = `openroll.directo.${ENLACE.sala}`;
    const guardado = almacen();
    const { c } = await conexion();
    c.conectar();
    const { rele, n } = await ofertaPublicada();
    await contestaElHost(rele, n, huellaAjena());
    await vi.waitFor(() => expect(respaldo()).toHaveLength(1));
    expect(JSON.parse(guardado[clave]).fallo).toBeGreaterThan(Date.now() - 5000);

    // Otra visita al poco: ni lo intenta (sin esperar a los relés)
    const otra = await conexion();
    otra.c.conectar();
    expect(respaldo()).toHaveLength(1);
    expect(reles()).toHaveLength(0);

    // Un fallo de hace más de media hora ya no cuenta, y si ahora funciona se borra la marca
    guardado[clave] = JSON.stringify({ fallo: Date.now() - 31 * 60_000 });
    const tercera = await conexion();
    tercera.c.conectar();
    const dos = await ofertaPublicada();
    await contestaElHost(dos.rele, dos.n);
    await vi.waitFor(() => expect(PCFalso.ultimo.remota).not.toBeNull());
    porJuego(PCFalso.ultimo, { tipo: "Reto", reto: "r" });
    await vi.waitFor(() => expect(guardado[clave]).toBeUndefined());
  });

  it("caída de la directa que había funcionado: pasa al respaldo y, en segundo plano, vuelve a lo directo sin cortar la sesión", async () => {
    almacen({ [`openroll.sala.${ENLACE.sala}`]: "tok" });
    const { c, esperas } = await conexion();
    esperas.splice(0, esperas.length, 20, 20, 20);
    c.conectar();
    const uno = await ofertaPublicada();
    await contestaElHost(uno.rele, uno.n);
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

    // En segundo plano se prueba otra vez lo directo, con una oferta nueva por los relés
    const antes = reles().length;
    await vi.waitFor(() => expect(reles().length).toBeGreaterThan(antes));
    const rele2 = reles()[antes];
    rele2.onopen!();
    const oferta = await descifrar(SECRETO, JSON.parse(rele2.enviados[1] as string)[1].content);
    await contestaElHost(rele2, oferta.n);
    await vi.waitFor(() => expect(PCFalso.ultimo).not.toBe(pc1));
    await vi.waitFor(() => expect(PCFalso.ultimo.remota).not.toBeNull());
    const pc2 = PCFalso.ultimo;
    expect(ws.cerrado).toBe(false); // hasta que la directa no habla, sigue el respaldo
    expect(c.estado.tipo).toBe("dentro");

    // Habla: se cambia (reconexión) y se saluda por la directa
    porJuego(pc2, { tipo: "Reto", reto: "r2" });
    await vi.waitFor(() => expect(ws.cerrado).toBe(true));
    expect(c.estado.tipo).toBe("reconectando");
    await vi.waitFor(() => expect(pc2.canales.juego.enviados.length).toBeGreaterThan(0));
    // Lo que diga el WebSocket viejo (p. ej. «has entrado desde otra pestaña») ya no cuenta
    ws.onmessage!({ data: JSON.stringify({ tipo: "Rechazado", motivo: "Has entrado desde otra pestaña" }) });
    ws.onclose!();
    expect(c.estado.tipo).toBe("reconectando");
    porJuego(pc2, bienvenido);
    await vi.waitFor(() => expect(c.estado.tipo).toBe("dentro"));
    expect(respaldo()).toHaveLength(1); // no se abrió ningún otro respaldo
  });

  it("si la prueba de volver a lo directo falla, sigue por el respaldo sin enterarse", async () => {
    almacen({ [`openroll.sala.${ENLACE.sala}`]: "tok" });
    const { c, esperas } = await conexion();
    esperas.splice(0, esperas.length, 20, 20, 20);
    c.conectar();
    const uno = await ofertaPublicada();
    await contestaElHost(uno.rele, uno.n);
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

    const antes = reles().length;
    await vi.waitFor(() => expect(reles().length).toBeGreaterThan(antes));
    const pcPrueba = PCFalso.ultimo; // el de esta prueba (luego habrá otras)
    const rele2 = reles()[antes];
    rele2.onopen!();
    const oferta = await descifrar(SECRETO, JSON.parse(rele2.enviados[1] as string)[1].content);
    await contestaElHost(rele2, oferta.n, huellaAjena()); // un host que no es el del enlace
    await vi.waitFor(() => expect(pcPrueba.cerrada).toBe(true)); // la prueba se suelta
    expect(ws.cerrado).toBe(false);
    expect(c.estado.tipo).toBe("dentro");
  });

  it("sin huella en el enlace, o sin WebRTC en el navegador, va directo al respaldo (sin tocar los relés)", async () => {
    const sin = await conexion({ ...ENLACE, huella: null });
    sin.c.conectar();
    expect(WebSocketFalso.ultimo.url).toBe("wss://x.trycloudflare.com/ws");
    expect(sin.c.estado.tipo).toBe("conectando");
    expect(reles()).toHaveLength(0);

    const { c } = await conexion();
    vi.stubGlobal("RTCPeerConnection", undefined);
    c.conectar();
    expect(WebSocketFalso.ultimo.url).toBe("wss://x.trycloudflare.com/ws");
    expect(reles()).toHaveLength(0);
  });
});
