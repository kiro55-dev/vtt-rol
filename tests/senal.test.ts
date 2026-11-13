// La señalización por relés de Nostr del lado del navegador (src/lib/red/senal.ts): eventos, etiquetas (con el vector
// que comparte con Rust) y la espera de la respuesta del host por varios relés.
import { describe, expect, it } from "vitest";
import { cifrar } from "$lib/red/codigo";
import { KIND, crear, crearAleatorio, etiqueta, firmaValida, idValido, leerEvento, pedirRespuesta, pedirRespuestaPorServidor } from "$lib/red/senal";

const SECRETO = "0123456789abcdef_-XYZ";

describe("eventos y etiquetas", () => {
  it("el evento que hace Rust (src-tauri/src/red/senal.rs) es válido y este lado hace el mismo", () => {
    const de_rust = {
      id: "d814104af9a6ac42f3f094ba78b7af7a3b1763fafc88803d55387ec5dc310202",
      pubkey: "1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f",
      created_at: 1700000000,
      kind: KIND,
      tags: [["t", "etiqueta"]],
      content: "hola",
      sig: "4f87b7ebaa9511c4b5c1204a154086ba752e0b4b835d8550898bec1e03373a505b4d73e302c6507cc0828d037ac2ee7de5d36bc3d86d56d44c21b5c7ad5eee86"
    };
    expect(idValido(de_rust)).toBe(true);
    expect(firmaValida(de_rust)).toBe(true);
    expect(crear(new Uint8Array(32).fill(1), new Uint8Array(32), 1_700_000_000, [["t", "etiqueta"]], "hola")).toEqual(de_rust);
    expect(etiqueta(SECRETO, "oferta")).toBe("3791f4b8ce20a3e2136792ead50c504b");
    expect(etiqueta(SECRETO, "respuesta")).toBe("a25020d1b759722a234a48235b24f009");
  });

  it("un evento con clave nueva es válido, y tocarlo lo rompe", () => {
    const a = crearAleatorio(1, [["t", "x"]], "c");
    const b = crearAleatorio(1, [["t", "x"]], "c");
    expect(a.pubkey).not.toBe(b.pubkey);
    expect(idValido(a) && firmaValida(a)).toBe(true);
    expect(idValido({ ...a, content: "otro" })).toBe(false);
    expect(firmaValida({ ...a, sig: "0".repeat(128) })).toBe(false);
    expect(firmaValida({ ...a, sig: "no es hex" })).toBe(false);
  });

  it("solo se lee un evento de un mensaje de relé", () => {
    const e = crearAleatorio(1, [["t", "x"]], "c");
    expect(leerEvento(JSON.stringify(["EVENT", "s", e]))).toEqual(e);
    expect(leerEvento(JSON.stringify(["OK", e.id, true, ""]))).toBeNull();
    expect(leerEvento(JSON.stringify(["EVENT", "s", { id: 3 }]))).toBeNull();
    expect(leerEvento("no es json")).toBeNull();
    expect(leerEvento(JSON.stringify(["EVENT", "s", { ...e, content: "x".repeat(70_000) }]))).toBeNull();
  });

});

class Rele {
  static todos: Rele[] = [];
  onopen: (() => void) | null = null;
  onmessage: ((e: { data: unknown }) => void) | null = null;
  onerror: (() => void) | null = null;
  onclose: (() => void) | null = null;
  enviados: string[] = [];
  cerrado = false;
  constructor(public url: string) {
    Rele.todos.push(this);
  }
  send(x: string) {
    this.enviados.push(x);
  }
  close() {
    this.cerrado = true;
  }
}

describe("pedirRespuesta", () => {
  const base = async () => {
    Rele.todos = [];
    const n = "uno";
    const codigoOferta = await cifrar(SECRETO, { tipo: "oferta", sdp: "v=0", t: Date.now(), n });
    return { n, codigoOferta, secreto: SECRETO, crearWebSocket: (u: string) => new Rele(u) as unknown as WebSocket };
  };
  const respuestaPara = async (n: string, tipo: "respuesta" | "oferta" = "respuesta") =>
    crearAleatorio(Math.floor(Date.now() / 1000), [["t", etiqueta(SECRETO, "respuesta")], ["x", n]], await cifrar(SECRETO, { tipo, sdp: "v=0", t: Date.now(), n: "r" }));

  it("publica la oferta en todos los relés (tras suscribirse a la respuesta) y devuelve la primera respuesta válida", async () => {
    const o = await base();
    const p = pedirRespuesta({ ...o, reles: ["wss://a", "wss://b"], topeMs: 2000 });
    expect(Rele.todos.map((r) => r.url)).toEqual(["wss://a", "wss://b"]);
    for (const r of Rele.todos) r.onopen!();
    const [req, ev] = Rele.todos[0].enviados.map((x) => JSON.parse(x));
    expect(req).toEqual(["REQ", "r", expect.objectContaining({ kinds: [KIND], "#t": [etiqueta(SECRETO, "respuesta")], "#x": ["uno"] })]);
    expect(ev[1].tags).toEqual([["t", etiqueta(SECRETO, "oferta")]]);
    expect(ev[1].content).toBe(o.codigoOferta);
    expect(firmaValida(ev[1])).toBe(true);

    const buena = await respuestaPara("uno");
    Rele.todos[1].onmessage!({ data: JSON.stringify(["EVENT", "r", buena]) });
    await expect(p).resolves.toBe(buena.content);
    expect(Rele.todos.every((r) => r.cerrado)).toBe(true); // y suelta los relés
  });

  it("ignora lo que no es para esta oferta, de otra etiqueta, con el id roto, que no descifra o que es una oferta", async () => {
    const o = await base();
    const p = pedirRespuesta({ ...o, reles: ["wss://a"], topeMs: 150 });
    const r = Rele.todos[0];
    r.onopen!();
    const dar = (e: unknown) => r.onmessage!({ data: JSON.stringify(["EVENT", "r", e]) });
    dar(await respuestaPara("otro"));
    dar({ ...(await respuestaPara("uno")), id: "0".repeat(64) });
    dar(crearAleatorio(Math.floor(Date.now() / 1000), [["t", "otra-etiqueta"], ["x", "uno"]], "x"));
    dar(crearAleatorio(Math.floor(Date.now() / 1000), [["t", etiqueta(SECRETO, "respuesta")], ["x", "uno"]], "no descifra"));
    dar(await respuestaPara("uno", "oferta")); // la oferta de alguien, no la respuesta del host
    r.onmessage!({ data: new ArrayBuffer(4) });
    await expect(p).rejects.toBe("El host no ha contestado");
  });

  it("falla con un texto si se agota el tiempo, si se cancela o si no hay relés", async () => {
    const o = await base();
    await expect(pedirRespuesta({ ...o, reles: ["wss://a"], topeMs: 50 })).rejects.toBe("El host no ha contestado");

    const ctl = new AbortController();
    const p = pedirRespuesta({ ...o, reles: ["wss://a"], topeMs: 5000, cancelar: ctl.signal });
    ctl.abort();
    await expect(p).rejects.toBe("Cancelado");

    await expect(pedirRespuesta({ ...o, reles: [] })).rejects.toBe("No hay relés a los que conectar");
    await expect(pedirRespuesta({ ...o, reles: ["wss://a"], cancelar: AbortSignal.abort() })).rejects.toBe("Cancelado");
  });

  it("un relé que no se puede abrir no impide usar los demás", async () => {
    const o = await base();
    let i = 0;
    const p = pedirRespuesta({
      ...o,
      reles: ["wss://roto", "wss://bueno"],
      topeMs: 2000,
      crearWebSocket: (u) => {
        if (i++ === 0) throw new Error("no se puede");
        return new Rele(u) as unknown as WebSocket;
      }
    });
    const r = Rele.todos[0];
    r.onopen!();
    const buena = await respuestaPara("uno");
    r.onmessage!({ data: JSON.stringify(["EVENT", "r", buena]) });
    await expect(p).resolves.toBe(buena.content);
  });
});

describe("pedirRespuestaPorServidor (la negociación por el servidor del host)", () => {
  const preparar = async () => {
    Rele.todos = [];
    const codigoOferta = await cifrar(SECRETO, { tipo: "oferta", sdp: "v=0", t: Date.now(), n: "uno" });
    return { secreto: SECRETO, url: "wss://host.test/ws", codigoOferta, crearWebSocket: (u: string) => new Rele(u) as unknown as WebSocket };
  };
  const respuesta = (tipo: "respuesta" | "oferta" = "respuesta") => cifrar(SECRETO, { tipo, sdp: "v=0", t: Date.now(), n: "r" });

  it("manda OfertaDirecta al abrir y devuelve el código de la respuesta, soltando el WebSocket", async () => {
    const o = await preparar();
    const p = pedirRespuestaPorServidor({ ...o, topeMs: 2000 });
    const ws = Rele.todos[0];
    expect(ws.url).toBe("wss://host.test/ws");
    ws.onopen!();
    expect(JSON.parse(ws.enviados[0])).toEqual({ tipo: "OfertaDirecta", codigo: o.codigoOferta });
    // Lo primero que manda el host es el reto: se ignora
    ws.onmessage!({ data: JSON.stringify({ tipo: "Reto", reto: "r" }) });
    const r = await respuesta();
    ws.onmessage!({ data: JSON.stringify({ tipo: "RespuestaDirecta", codigo: r }) });
    await expect(p).resolves.toBe(r);
    expect(ws.cerrado).toBe(true);
  });

  it("falla con un texto si el host dice que no, no conecta, se cierra, no contesta o se cancela", async () => {
    const o = await preparar();
    let p = pedirRespuestaPorServidor({ ...o, topeMs: 2000 });
    Rele.todos[0].onmessage!({ data: JSON.stringify({ tipo: "OfertaDirectaError", error: "Esta sala no ofrece conexión directa" }) });
    await expect(p).rejects.toBe("Esta sala no ofrece conexión directa");

    p = pedirRespuestaPorServidor({ ...o, topeMs: 2000 });
    Rele.todos[1].onerror!();
    await expect(p).rejects.toBe("No se pudo conectar con el servidor");

    p = pedirRespuestaPorServidor({ ...o, topeMs: 2000 });
    Rele.todos[2].onclose!();
    await expect(p).rejects.toBe("El servidor cerró la conexión");

    p = pedirRespuestaPorServidor({ ...o, topeMs: 2000 });
    Rele.todos[3].onmessage!({ data: JSON.stringify({ tipo: "Rechazado", motivo: "Versión distinta" }) });
    await expect(p).rejects.toBe("Versión distinta");

    // Abre pero no contesta: salta la espera de la respuesta
    const lento = pedirRespuestaPorServidor({ ...o, topeMs: 40 });
    Rele.todos[Rele.todos.length - 1].onopen!();
    await expect(lento).rejects.toBe("El servidor no ha contestado");
    // Ni abre: salta la espera de la conexión
    await expect(pedirRespuestaPorServidor({ ...o, topeConexionMs: 40 })).rejects.toBe("El servidor no ha abierto la conexión");

    const ctl = new AbortController();
    p = pedirRespuestaPorServidor({ ...o, topeMs: 2000, cancelar: ctl.signal });
    ctl.abort();
    await expect(p).rejects.toBe("Cancelado");
    await expect(pedirRespuestaPorServidor({ ...o, cancelar: AbortSignal.abort() })).rejects.toBe("Cancelado");
    await expect(pedirRespuestaPorServidor({ ...o, crearWebSocket: () => { throw new Error("no"); } })).rejects.toBe("No se pudo conectar con el servidor");
  });

  it("no se fía de una respuesta que no descifra con el secreto de la sala o que es una oferta", async () => {
    const o = await preparar();
    const p = pedirRespuestaPorServidor({ ...o, topeMs: 150 });
    const ws = Rele.todos[0];
    ws.onmessage!({ data: JSON.stringify({ tipo: "RespuestaDirecta", codigo: "no descifra" }) });
    ws.onmessage!({ data: JSON.stringify({ tipo: "RespuestaDirecta", codigo: await respuesta("oferta") }) });
    ws.onmessage!({ data: new ArrayBuffer(4) });
    ws.onmessage!({ data: "no es json" });
    ws.onopen!();
    await expect(p).rejects.toBe("El servidor no ha contestado");
  });

  it("el tiempo de la respuesta cuenta desde que se envía la oferta, no desde que se pidió la conexión", async () => {
    const o = await preparar();
    const p = pedirRespuestaPorServidor({ ...o, topeConexionMs: 1000, topeMs: 200 });
    const ws = Rele.todos[0];
    await new Promise((r) => setTimeout(r, 120)); // conectar tardó: más que la respuesta, menos que el tope de conexión
    ws.onopen!();
    await new Promise((r) => setTimeout(r, 120)); // otros 120 ms: en total 240 > 200, pero la respuesta lleva solo 120
    const r = await respuesta();
    ws.onmessage!({ data: JSON.stringify({ tipo: "RespuestaDirecta", codigo: r }) });
    await expect(p).resolves.toBe(r);
  });
});
