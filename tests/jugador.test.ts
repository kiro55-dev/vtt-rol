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
  bufferedAmount = 0;
  binarios: Uint8Array[] = [];
  send(t: string | Uint8Array) {
    if (typeof t === "string") this.enviados.push(JSON.parse(t));
    else this.binarios.push(t);
  }
  close() {}
  /** El host dice algo. */
  recibir(m: object) {
    this.onmessage?.({ data: JSON.stringify(m) });
  }
}

const ENLACE = { tunel: "x.trycloudflare.com", sala: "AbCdEfGhIjKlMnOpQrStUv", secreto: "0123456789abcdef_-XYZ", mismoOrigen: false, seguro: true, huella: null };

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

describe("conexión del jugador: nombre que ya existe", () => {
  afterEach(() => vi.unstubAllGlobals());

  /** Un navegador sin token: llega hasta que el host le pide el nombre. */
  async function pidiendoNombre() {
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
    ws.recibir({ tipo: "Identificate", pide_contrasena: false });
    return { c, ws };
  }

  it("si el host dice que el nombre ya existe, el estado lo sabe; y entrar igualmente lo manda con la marca", async () => {
    const { c, ws } = await pidiendoNombre();
    expect(c.estado).toMatchObject({ tipo: "identificate", nombreExistente: null });

    c.unirse("  Ana ", "");
    expect(ws.enviados.filter((m) => m.tipo === "Unirse").at(-1)).toEqual({ tipo: "Unirse", nombre: "Ana" }); // sin la marca
    ws.recibir({ tipo: "Identificate", pide_contrasena: false, nombre_existente: "Ana" });
    expect(c.estado).toMatchObject({ tipo: "identificate", nombreExistente: "Ana", error: null });

    c.unirse("Ana", "secreta", true);
    expect(ws.enviados.filter((m) => m.tipo === "Unirse").at(-1)).toEqual({ tipo: "Unirse", nombre: "Ana", contrasena: "secreta", entrar_igualmente: true });
    ws.recibir({ tipo: "Bienvenido", yo: { id: 1, nombre: "Ana" }, sesion: "S", token: "t" });
    expect(c.estado.tipo).toBe("dentro");
  });

  it("si el host dice que ya hay alguien con ese nombre, es un error y no se ofrece nada", async () => {
    const { c, ws } = await pidiendoNombre();
    ws.recibir({ tipo: "Identificate", pide_contrasena: false, error: "Ya hay un jugador con ese nombre: elige otro" });
    expect(c.estado).toMatchObject({ tipo: "identificate", nombreExistente: null, error: "Ya hay un jugador con ese nombre: elige otro" });
  });
});

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

  it("el arrastre en vivo va como un `Aviso` sin respuesta (ni espera ni cola), con el número de orden", async () => {
    const { c, ws } = await conectado();
    const antes = ws.enviados.filter((m) => m.tipo === "Pedir").length;
    const { avisoArrastre, avisoArrastreFin } = await import("$lib/avisosTablero");
    const a = avisoArrastre(1, [{ id_instancia: 4, casilla: { x: 2, y: 3 } }]);
    const f = avisoArrastreFin(1, [4]);
    await expect(c.responder("avisar_tablero", { aviso: a })).resolves.toBeNull();
    await expect(c.responder("avisar_tablero", { aviso: f })).resolves.toBeNull();
    const avisos = ws.enviados.filter((m) => m.tipo === "Aviso");
    expect(avisos.map((m) => m.aviso)).toEqual([a, f]);
    expect("Arrastre" in a && "ArrastreFin" in f && f.ArrastreFin.n! > a.Arrastre.n!).toBe(true);
    expect(ws.enviados.filter((m) => m.tipo === "Pedir").length).toBe(antes);
  });

  it("el recorrido (flecha del botón derecho) también va como `Aviso`, no como petición con cola y respuesta", async () => {
    const { c, ws } = await conectado();
    const antes = ws.enviados.filter((m) => m.tipo === "Pedir").length;
    const { avisoRecorrido, avisoRecorridoBorrado } = await import("$lib/avisosTablero");
    const a = avisoRecorrido(1, [{ x: 1, y: 1 }, { x: 2, y: 1 }], [10, 20, 30]);
    const b = avisoRecorridoBorrado(1);
    await expect(c.responder("avisar_tablero", { aviso: a })).resolves.toBeNull();
    await expect(c.responder("avisar_tablero", { aviso: b })).resolves.toBeNull();
    expect(ws.enviados.filter((m) => m.tipo === "Aviso").map((m) => m.aviso)).toEqual([a, b]);
    expect(ws.enviados.filter((m) => m.tipo === "Pedir").length).toBe(antes);
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

  it("al entrar recupera del host sus comandos de dados anteriores, y lo que escriba mientras tanto se conserva", async () => {
    const { c, ws } = await conectado();
    const pedida = ws.enviados.filter((m) => m.tipo === "Pedir").find((m) => m.peticion.cmd === "obtener_historial_personal");
    expect(pedida).toBeTruthy();
    void c.responder("tirar_dado", { comando: "1d20" }); // escrito antes de que llegue la respuesta
    ws.recibir({ tipo: "Respuesta", id: pedida.id, ok: ["2d6", "1d8 + 1"] });
    await vi.waitFor(async () => expect(await c.responder("obtener_historial_personal", {})).toEqual(["2d6", "1d8 + 1", "1d20"]));
  });

  it("sube la foto al host por la conexión, en trozos con la extensión del archivo, y devuelve la ruta", async () => {
    const { c, ws } = await conectado();
    const bytes = new Uint8Array(40_000).map((_, i) => i % 256);
    const archivo = Object.assign(new Blob([bytes]), { name: "mi foto.PNG" }) as unknown as File;
    const subida = c.subirImagen(archivo);
    await vi.waitFor(() => expect(ws.binarios.length).toBe(3)); // 16 KiB + 16 KiB + el resto
    const aviso = ws.enviados.find((m) => m.tipo === "SubirImagen");
    expect(aviso).toEqual({ tipo: "SubirImagen", id: aviso.id, ext: "PNG", tam: 40_000 });
    const recibido = ws.binarios.flatMap((t) => [...t.subarray(5)]);
    expect(recibido).toEqual([...bytes]);
    ws.recibir({ tipo: "SubidaOk", id: aviso.id, ruta: "imagenes/abc.png" });
    await expect(subida).resolves.toBe("imagenes/abc.png");

    // Un rechazo del host se enseña tal cual
    const otra = c.subirImagen(archivo);
    await vi.waitFor(() => expect(ws.enviados.filter((m) => m.tipo === "SubirImagen").length).toBe(2));
    ws.recibir({ tipo: "SubidaError", id: ws.enviados.filter((m) => m.tipo === "SubirImagen")[1].id, error: "El archivo no es una imagen" });
    await expect(otra).rejects.toBe("El archivo no es una imagen");
  });

  it("sin Service Worker, las imágenes usan el respaldo HTTP del túnel con la sesión", async () => {
    const { c } = await conectado();
    expect(c.urlImagen("imagenes/a.png")).toBe("https://x.trycloudflare.com/imagen?ruta=imagenes%2Fa.png&sesion=S");
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

  it("puede abrir en el editor una ficha de su banco y crear una nueva (el host le da el borrador)", async () => {
    const { partida, llamadas } = await comoJugador();
    partida.banco = [{ id: 10, nombre: "Ana" }] as any;
    expect(await partida.abrirEditorEntidad(partida.banco[0])).toBe(true);
    expect(partida.entidadEnEdicion?.id).toBe(10);
    expect(partida.editorEsBorrador).toBe(false);

    // Una ficha nueva: se pide un borrador (que contesta el host)
    partida.entidadEnEdicion = null;
    expect(await partida.abrirEditorEntidad(null)).toBe(true);
    expect(llamadas).toContain("iniciar_borrador_entidad");
    expect(partida.editorEsBorrador).toBe(true);
  });

  it("el jugador ve el mapa del host y no elige: entrar el host en uno privado no cambia nada", async () => {
    vi.resetModules();
    vi.doMock("$lib/modo", () => ({ esJugador: true }));
    vi.doMock("$lib/red/conexion.svelte", () => ({ conexion: null }));
    const zona = (id: number) => ({ id, nombre: `Z${id}`, orden: id });
    const mapa = (id: number, id_zona: number) => ({ id, id_zona, nombre: `M${id}`, orden: id, url_imagen_fondo: null, ancho: 100, alto: 100, rejilla: 10, color_opacidad: [255, 255, 255, 76], grosor: 1, compartido: true });
    const mapas = [mapa(1, 1), mapa(2, 1), mapa(3, 2)];
    let visible: any = mapas[0];
    vi.doMock("$lib/backend", () => ({
      invoke: async (cmd: string, args: any) => {
        if (cmd === "obtener_zonas") return [zona(1), zona(2)];
        if (cmd === "obtener_mapas_zona") return mapas.filter((m) => m.id_zona === args.zonaId);
        if (cmd === "obtener_mapa_visible") return visible;
        return [];
      },
      listen: async () => () => {},
      emitirLocal: () => {},
      EVENTO_PARTIDA_CAMBIADA: "partida-cambiada"
    }));
    const { partida } = await import("$lib/partida.svelte");
    await partida.iniciar();
    expect(partida.mapaActual?.id).toBe(1);

    // Pulsar otra miniatura no cambia lo que ve
    await partida.seleccionarMapa(mapas[1] as any);
    expect(partida.mapaActual?.id).toBe(1);

    // Mirar otra zona en la lista tampoco le cambia el mapa
    await partida.seleccionarZona(2);
    expect(partida.mapas.map((m) => m.id)).toEqual([3]);
    expect(partida.mapaActual?.id).toBe(1);

    // El host abre otro mapa compartido (de otra zona): lo ve, y su lista salta a esa zona
    visible = mapas[2];
    await partida.alCambiarPartida({ mapas: true });
    expect(partida.mapaActual?.id).toBe(3);
    expect(partida.zonaId).toBe(2);

    // Si el host entra en uno que no ve, Rust sigue diciendo el mismo (el último compartido): sin cambios
    await partida.alCambiarPartida({ mapas: true });
    expect(partida.mapaActual?.id).toBe(3);
    vi.doUnmock("$lib/backend");
  });

  it("un aviso se enseña y se quita solo", async () => {
    vi.useFakeTimers();
    try {
      const { partida } = await comoJugador();
      partida.avisar("algo falló");
      expect(partida.avisos).toBe("algo falló");
      vi.advanceTimersByTime(6001);
      expect(partida.avisos).toBe("");
    } finally {
      vi.useRealTimers();
    }
  });
});
