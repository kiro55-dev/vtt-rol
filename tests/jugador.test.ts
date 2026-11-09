// Pruebas de cómo usa la app un JUGADOR: la conexión con el host (acciones y respuestas) y qué puede
// cambiar de las fichas (src/lib/red/conexion.svelte.ts, src/lib/partida.svelte.ts).
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// ---- Un WebSocket falso: guarda lo que se le envía y deja simular lo que dice el host ----
class WebSocketFalso {
  static OPEN = 1;
  static ultimo: WebSocketFalso;
  readyState = 1;
  enviados: any[] = [];
  onmessage: ((e: { data: string }) => void) | null = null;
  onclose: (() => void) | null = null;
  constructor(public url: string) {
    WebSocketFalso.ultimo = this;
  }
  send(t: string) {
    this.enviados.push(JSON.parse(t));
  }
  close() {}
  /** El host dice algo. */
  recibir(m: object) {
    this.onmessage?.({ data: JSON.stringify(m) });
  }
}

const ENLACE = { tunel: "x.trycloudflare.com", sala: "AbCdEfGhIjKlMnOpQrStUv", secreto: "0123456789abcdef_-XYZ" };

async function conectado() {
  vi.resetModules();
  vi.doMock("@tauri-apps/api/core", () => ({ invoke: async () => null, convertFileSrc: (p: string) => p }));
  vi.doMock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));
  vi.stubGlobal("WebSocket", WebSocketFalso);
  const { Conexion } = await import("$lib/red/conexion.svelte");
  const c = new Conexion(ENLACE);
  c.conectar();
  const ws = WebSocketFalso.ultimo;
  ws.recibir({ tipo: "Reto", reto: "r" });
  await vi.waitFor(() => expect(ws.enviados.find((m) => m.tipo === "Hola")).toBeTruthy());
  ws.recibir({ tipo: "Bienvenido", yo: { id: 1, nombre: "Ana" }, sesion: "S" });
  return { c, ws };
}

const peticion = (ws: WebSocketFalso) => ws.enviados.filter((m) => m.tipo === "Pedir").at(-1);

describe("conexión del jugador: pedirle acciones al host", () => {
  beforeEach(() => vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] }));
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("una acción se envía tal cual (`cmd` + argumentos) y se resuelve con la respuesta del host", async () => {
    const { c, ws } = await conectado();
    const p = c.responder("mover_instancia", { idInstancia: 5, destino: { id_mapa: 1, casilla: { x: 2, y: 3 } } });
    const pedida = peticion(ws);
    expect(pedida.peticion).toEqual({ cmd: "mover_instancia", idInstancia: 5, destino: { id_mapa: 1, casilla: { x: 2, y: 3 } } });
    ws.recibir({ tipo: "Respuesta", id: pedida.id, ok: { hecho: true } });
    await expect(p).resolves.toEqual({ hecho: true });

    // Una acción sin resultado (el host contesta `null`)
    const q = c.responder("rotar_instancia", { idInstancia: 5 });
    ws.recibir({ tipo: "Respuesta", id: peticion(ws).id, ok: null });
    await expect(q).resolves.toBeNull();
  });

  it("si el host la rechaza, el error llega tal cual", async () => {
    const { c, ws } = await conectado();
    const p = c.responder("mover_instancia", { idInstancia: 9 });
    ws.recibir({ tipo: "Respuesta", id: peticion(ws).id, error: "Esa ficha no es tuya" });
    await expect(p).rejects.toBe("Esa ficha no es tuya");
  });

  it("las respuestas se emparejan por id aunque lleguen desordenadas", async () => {
    const { c, ws } = await conectado();
    const a = c.responder("tirar_dado", { comando: "1d6" });
    const primera = peticion(ws).id;
    const b = c.responder("tirar_dado", { comando: "1d8" });
    const segunda = peticion(ws).id;
    expect(segunda).not.toBe(primera);
    ws.recibir({ tipo: "Respuesta", id: segunda, ok: "B" });
    ws.recibir({ tipo: "Respuesta", id: primera, ok: "A" });
    await expect(a).resolves.toBe("A");
    await expect(b).resolves.toBe("B");
  });

  it("sin respuesta del host, falla a los 30 s; y al cortarse la conexión, las pendientes fallan", async () => {
    const { c, ws } = await conectado();
    const lenta = c.responder("tirar_dado", { comando: "1d6" });
    vi.advanceTimersByTime(30_001);
    await expect(lenta).rejects.toBe("El host no ha contestado");

    const cortada = c.responder("tirar_dado", { comando: "1d6" });
    ws.onclose?.();
    await expect(cortada).rejects.toBe("Sin conexión con la sala");
    // Y mientras no hay conexión, ni se intenta
    await expect(c.responder("tirar_dado", { comando: "1d6" })).rejects.toBe("Sin conexión con la sala");
  });

  it("el foco es de cada uno: no se envía al host; ping y recorrido sí", async () => {
    const { c, ws } = await conectado();
    const antes = ws.enviados.length;
    await expect(c.responder("avisar_tablero", { aviso: { Foco: { ids_instancias: [1, 2] } } })).resolves.toBeNull();
    expect(ws.enviados.length).toBe(antes);

    void c.responder("avisar_tablero", { aviso: { Ping: { id_mapa: 1, casilla: { x: 1, y: 1 } } } });
    expect(peticion(ws).peticion.cmd).toBe("avisar_tablero");
  });

  it("las lecturas no van al host: salen de la réplica, y lo que no está permitido se rechaza", async () => {
    const { c, ws } = await conectado();
    ws.recibir({
      tipo: "Vista",
      vista: { zonas: [{ id: 1, nombre: "Z", orden: 0 }], mapas: [], instancias: [], entidades: [], definiciones: [], historial_tiradas: [] }
    });
    const pedidasAntes = ws.enviados.filter((m) => m.tipo === "Pedir").length;
    await expect(c.responder("obtener_zonas", {})).resolves.toEqual([{ id: 1, nombre: "Z", orden: 0 }]);
    await expect(c.responder("crear_zona", {})).rejects.toBeTruthy();
    await expect(c.responder("limpiar_chat", { comando: "clean" })).rejects.toBeTruthy();
    expect(ws.enviados.filter((m) => m.tipo === "Pedir").length).toBe(pedidasAntes);
  });

  it("recuerda lo que el jugador escribe en el chat para las flechas ↑ ↓ (sin repetir el último)", async () => {
    const { c } = await conectado();
    void c.responder("tirar_dado", { comando: " 1d6 " });
    void c.responder("tirar_dado", { comando: "1d6" });
    void c.responder("tirar_dado", { comando: "2d8 + 1" });
    await expect(c.responder("obtener_historial_personal", {})).resolves.toEqual(["1d6", "2d8 + 1"]);
  });

  it("sube la foto al host con la sesión y la extensión del archivo, y devuelve la ruta", async () => {
    const { c } = await conectado();
    const peticiones: { url: string; metodo?: string }[] = [];
    vi.stubGlobal("fetch", async (url: string, init?: { method?: string }) => {
      peticiones.push({ url, metodo: init?.method });
      return { ok: true, json: async () => ({ ruta: "imagenes/abc.png" }), text: async () => "" };
    });
    const archivo = { name: "mi foto.PNG" } as File;
    await expect(c.subirImagen(archivo)).resolves.toBe("imagenes/abc.png");
    expect(peticiones[0]).toEqual({ url: "https://x.trycloudflare.com/subir?sesion=S&ext=PNG", metodo: "POST" });

    // Un rechazo del host se enseña tal cual
    vi.stubGlobal("fetch", async () => ({ ok: false, status: 400, text: async () => "El archivo no es una imagen" }));
    await expect(c.subirImagen(archivo)).rejects.toBe("El archivo no es una imagen");
  });
});

describe("un jugador solo cambia las fichas que son suyas", () => {
  async function comoJugador() {
    vi.resetModules();
    vi.doMock("$lib/modo", () => ({ esJugador: true }));
    vi.doMock("$lib/red/conexion.svelte", () => ({ conexion: null }));
    const llamadas: string[] = [];
    vi.doMock("$lib/backend", () => ({
      invoke: async (cmd: string) => {
        llamadas.push(cmd);
        return [];
      },
      listen: async () => () => {},
      emitirLocal: () => {},
      EVENTO_PARTIDA_CAMBIADA: "partida-cambiada"
    }));
    const { partida } = await import("$lib/partida.svelte");
    return { partida, llamadas };
  }

  const instancia = (id: number, idFicha: number, vida = 10) =>
    ({
      id_instancia: id, id_mapa: 1, coordenadas: "NoPuesto", rotacion: 0, orden: id,
      valores_actuales: [{ id_definicion: 1, valor: vida }],
      base: { id: idFicha, nombre: "F", parametros: [{ id_definicion: 1, valor: { Numero: 20 }, publico: true }] }
    }) as any;

  afterEach(() => {
    vi.doUnmock("$lib/modo");
    vi.doUnmock("$lib/backend");
    vi.doUnmock("$lib/red/conexion.svelte");
  });

  it("controla las instancias de las fichas de su banco, no las demás", async () => {
    const { partida } = await comoJugador();
    partida.banco = [{ id: 10 }] as any; // su vista solo trae sus fichas
    expect(partida.controla(instancia(1, 10))).toBe(true);
    expect(partida.controla(instancia(2, 11))).toBe(false);
  });

  it("no cambia el valor (la vida) de una ficha ajena, y sí el de la suya", async () => {
    const { partida } = await comoJugador();
    partida.banco = [{ id: 10 }] as any;
    partida.instancias = [instancia(1, 10), instancia(2, 11)];
    partida.cambiarValorActual(2, 1, -3);
    expect(partida.valoresPendientes).toEqual({});
    partida.cambiarValorActual(1, 1, -3);
    expect(partida.valoresPendientes).toEqual({ "1:1": -3 });
    await partida.guardarValoresPendientes();
  });

  it("puede abrir en el editor una ficha de su banco, pero no crear una nueva", async () => {
    const { partida } = await comoJugador();
    partida.banco = [{ id: 10, nombre: "Ana" }] as any;
    expect(await partida.abrirEditorEntidad(null)).toBe(false);
    expect(partida.entidadEnEdicion).toBeNull();
    expect(await partida.abrirEditorEntidad(partida.banco[0])).toBe(true);
    expect(partida.entidadEnEdicion?.id).toBe(10);
  });
});
