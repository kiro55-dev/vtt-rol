// Pruebas del estado compartido del front contra un "Rust" falso en memoria.
import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";

type Args = Record<string, any>;

// ---- Backend falso: imita los comandos de Tauri que usa el store ----
function crearBackend() {
  const clon = <T>(x: T): T => structuredClone(x);
  const mapa = (id: number, id_zona: number, orden: number) => ({
    id, id_zona, nombre: String(id), url_imagen_fondo: null as string | null,
    ancho: 1000, alto: 1000, rejilla: 50, color_opacidad: [255, 255, 255, 76], grosor: 1, orden
  });
  const b = {
    zonas: [
      { id: 1, nombre: "A", orden: 0 },
      { id: 2, nombre: "B", orden: 1 },
      { id: 3, nombre: "C", orden: 2 }
    ],
    mapas: [mapa(1, 1, 0), mapa(2, 1, 1), mapa(3, 2, 0), mapa(4, 3, 0)],
    banco: [{ id: 1, nombre: "Orco", zona_id: 1 }, { id: 2, nombre: "Lobo", zona_id: 2 }],
    instancias: [
      {
        id_instancia: 1, id_mapa: 1, valores_actuales: [{ id_definicion: 1, valor: 10 }], orden: 0, rotacion: 0,
        base: { nombre: "Orco", tipo: "Enemigo", parametros: [{ id_definicion: 1, valor: { Numero: 18 }, publico: false }] }
      },
      {
        id_instancia: 2, id_mapa: 3, valores_actuales: [] as { id_definicion: number; valor: number }[], orden: 1, rotacion: 0,
        base: { nombre: "Lobo", tipo: "Enemigo", parametros: [{ id_definicion: 1, valor: { Numero: 5 }, publico: false }] }
      }
    ],
    definiciones: [
      { id: 1, clave: "Vida", valor_por_defecto: { Numero: 10 }, tiene_maximo: true, representar_mapa: null, visibilidad: "Variable", unidad: null }
    ],
    partidas: ["A", "B"] as string[],
    actual: "A",
    llamadas: [] as { cmd: string; args: Args }[],
    invoke: async (cmd: string, args: Args = {}): Promise<any> => {
      b.llamadas.push({ cmd, args: clon(args) });
      switch (cmd) {
        case "obtener_zonas": return clon(b.zonas);
        case "obtener_mapas_zona": return clon(b.mapas.filter((m) => m.id_zona === args.zonaId));
        case "obtener_banco_entidades": return clon(b.banco);
        case "obtener_instancias_activas": return clon(b.instancias);
        case "obtener_dir_datos": return "/datos";
        case "obtener_definiciones_parametros": return clon(b.definiciones);
        case "crear_definicion_parametro": {
          if (b.definiciones.some((d) => d.clave === args.definicion.clave)) throw "Ya existe un parámetro llamado";
          const d = { ...clon(args.definicion), id: Math.max(...b.definiciones.map((x) => x.id)) + 1 };
          b.definiciones.push(d);
          return clon(d);
        }
        case "modificar_definicion_parametro": b.definiciones = b.definiciones.map((d) => (d.id === args.definicion.id ? clon(args.definicion) : d)); return;
        case "obtener_partidas": return { actual: b.actual, partidas: [...b.partidas] };
        case "abrir_partida": case "crear_partida": {
          const existe = b.partidas.includes(args.nombre);
          if (cmd === "abrir_partida" && !existe) throw "Esa partida no existe";
          if (cmd === "crear_partida" && existe) throw "Ya existe una partida llamada «" + args.nombre + "»";
          if (!existe) b.partidas.push(args.nombre);
          // Imita a Rust: la otra partida tiene sus propios datos (aquí, solo una zona y sin mapas)
          b.actual = args.nombre;
          b.zonas = [{ id: 9, nombre: "Otra", orden: 0 }];
          b.mapas = []; b.banco = []; b.instancias = [];
          return;
        }
        case "borrar_definicion_parametro": b.definiciones = b.definiciones.filter((d) => d.id !== args.id); return;
        case "reordenar_definiciones_parametros": b.definiciones.sort((x, y) => args.ids.indexOf(x.id) - args.ids.indexOf(y.id)); return;
        case "importar_imagen": return "imagenes/" + String(args.origen).split("/").pop();
        case "crear_zona": {
          const z = { id: Math.max(...b.zonas.map((x) => x.id)) + 1, nombre: "nueva", orden: b.zonas.length };
          b.zonas.push(z);
          return clon(z);
        }
        case "modificar_zona": b.zonas = b.zonas.map((z) => (z.id === args.zona.id ? clon(args.zona) : z)); return;
        case "eliminar_zona": {
          const quitados = b.mapas.filter((m) => m.id_zona === args.zona.id).map((m) => m.id);
          b.zonas = b.zonas.filter((z) => z.id !== args.zona.id);
          b.mapas = b.mapas.filter((m) => m.id_zona !== args.zona.id);
          b.instancias = b.instancias.filter((i) => !quitados.includes(i.id_mapa));
          b.banco = b.banco.filter((e) => e.zona_id !== args.zona.id);
          return;
        }
        case "crear_mapa": {
          const m = mapa(Math.max(...b.mapas.map((x) => x.id)) + 1, args.idZona, 99);
          m.url_imagen_fondo = args.urlImagenFondo;
          b.mapas.push(m);
          return clon(m);
        }
        case "modificar_mapa": b.mapas = b.mapas.map((m) => (m.id === args.mapa.id ? clon(args.mapa) : m)); return;
        case "borrar_mapa": b.mapas = b.mapas.filter((m) => m.id !== args.mapa.id); return;
        // Instancias: imita lo justo de Rust (las reglas de verdad se prueban en instancias.rs)
        case "instanciar_entidad": {
          const base = b.banco.find((e) => e.id === args.idEntidad);
          if (!base) throw "Esa ficha no existe";
          const i: any = {
            id_instancia: Math.max(0, ...b.instancias.map((x) => x.id_instancia)) + 1,
            id_mapa: args.destino?.id_mapa ?? null,
            coordenadas: args.destino ? { Casilla: args.destino.casilla } : "NoPuesto",
            valores_actuales: [], base: { ...clon(base), tipo: "Enemigo", parametros: [] },
            orden: Math.max(-1, ...b.instancias.map((x) => x.orden)) + 1, rotacion: 0
          };
          b.instancias.push(i);
          return clon(i);
        }
        case "mover_instancia": {
          const i: any = b.instancias.find((x) => x.id_instancia === args.idInstancia);
          if (!i) throw "Esa instancia no existe";
          i.id_mapa = args.destino?.id_mapa ?? null;
          i.coordenadas = args.destino ? { Casilla: args.destino.casilla } : "NoPuesto";
          return;
        }
        case "borrar_instancia": b.instancias = b.instancias.filter((x) => x.id_instancia !== args.idInstancia); return;
        case "rotar_instancia": {
          const i = b.instancias.find((x) => x.id_instancia === args.idInstancia)!;
          i.rotacion = (i.rotacion + 1) % 4;
          return;
        }
        case "reordenar_instancias":
          for (const i of b.instancias) i.orden = args.ids.indexOf(i.id_instancia);
          return;
        case "modificar_valor_actual": {
          const i = b.instancias.find((x) => x.id_instancia === args.idInstancia)!;
          const v = i.valores_actuales.find((x) => x.id_definicion === args.idDefinicion);
          if (v) v.valor = Math.max(0, v.valor + args.delta);
          return;
        }
        default: throw new Error("comando desconocido: " + cmd);
      }
    }
  };
  return b;
}

let backend: ReturnType<typeof crearBackend>;

async function nuevoStore() {
  vi.resetModules();
  backend = crearBackend();
  vi.doMock("@tauri-apps/api/core", () => ({
    invoke: (c: string, a?: Args) => backend.invoke(c, a),
    convertFileSrc: (p: string) => "asset://" + p
  }));
  const { partida } = await import("$lib/partida.svelte");
  await partida.iniciar();
  return partida;
}

const llamadas = (cmd: string) => backend.llamadas.filter((l) => l.cmd === cmd);

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("arranque", () => {
  it("abre la primera zona, su primer mapa, y toma la rejilla de ese mapa", async () => {
    backend = crearBackend();
    backend.mapas[0].rejilla = 70;
    backend.mapas[0].color_opacidad = [255, 0, 0, 128];
    const partida = await (async () => {
      vi.resetModules();
      vi.doMock("@tauri-apps/api/core", () => ({ invoke: (c: string, a?: Args) => backend.invoke(c, a), convertFileSrc: (p: string) => p }));
      const { partida } = await import("$lib/partida.svelte");
      await partida.iniciar();
      return partida;
    })();
    expect(partida.zonaId).toBe(1);
    expect(partida.mapaActual?.id).toBe(1);
    expect(partida.rejilla.tamano).toBe(70);
    expect(partida.rejilla.color).toBe("#ff0000");
    expect(partida.banco).toHaveLength(2);
  });
});

describe("punto 4: borrar la zona seleccionada", () => {
  it("pasa a la zona vecina y muestra SUS mapas (antes se quedaba con los de la zona borrada)", async () => {
    const partida = await nuevoStore();
    await partida.borrarZona(partida.zonas.find((z) => z.id === 1)!);

    expect(partida.zonaId).toBe(2);
    expect(partida.mapas.map((m) => m.id)).toEqual([3]);
    expect(partida.mapaActual?.id).toBe(3);
    expect(partida.zonas.map((z) => z.id)).toEqual([2, 3]);
  });

  it("recarga las fichas y el banco, que Rust modifica al borrar la zona", async () => {
    const partida = await nuevoStore();
    await partida.borrarZona(partida.zonas.find((z) => z.id === 1)!);
    expect(partida.instancias.map((i) => i.id_instancia)).toEqual([2]); // la del mapa 1 ya no está
    expect(partida.banco.map((e) => e.nombre)).toEqual(["Lobo"]);
  });

  it("si era la última zona, se queda sin zona ni mapa, y no permite crear mapas", async () => {
    const partida = await nuevoStore();
    for (const id of [1, 2, 3]) await partida.borrarZona(partida.zonas.find((z) => z.id === id)!);

    expect(partida.zonaId).toBeNull();
    expect(partida.mapas).toEqual([]);
    expect(partida.mapaActual).toBeNull();

    await partida.crearMapa(null);
    expect(llamadas("crear_mapa")).toHaveLength(0);
  });

  it("al borrar la zona del final, pasa a la anterior", async () => {
    const partida = await nuevoStore();
    await partida.seleccionarZona(3);
    await partida.borrarZona(partida.zonas.find((z) => z.id === 3)!);
    expect(partida.zonaId).toBe(2);
  });

  it("borrar OTRA zona no mueve la selección ni el mapa abierto", async () => {
    const partida = await nuevoStore();
    await partida.borrarZona(partida.zonas.find((z) => z.id === 3)!);
    expect(partida.zonaId).toBe(1);
    expect(partida.mapaActual?.id).toBe(1);
  });

  it("descarta los cambios de rejilla pendientes del mapa que desaparece (no escribe un mapa borrado)", async () => {
    const partida = await nuevoStore();
    partida.rejilla.tamano = 120;
    partida.marcarRejillaModificada();

    await partida.borrarZona(partida.zonas.find((z) => z.id === 1)!);
    await vi.advanceTimersByTimeAsync(10_000);

    expect(llamadas("modificar_mapa")).toHaveLength(0);
  });
});

describe("rejilla y cambios de mapa", () => {
  it("al cambiar de mapa guarda primero la rejilla pendiente del que se abandona", async () => {
    const partida = await nuevoStore();
    partida.rejilla.tamano = 90;
    partida.marcarRejillaModificada();

    await partida.seleccionarMapa(partida.mapas.find((m) => m.id === 2)!);

    expect(backend.mapas.find((m) => m.id === 1)!.rejilla).toBe(90);
    expect(partida.mapaActual?.id).toBe(2);
    expect(partida.rejilla.tamano).toBe(50); // la del mapa 2, no la del 1
  });

  it("un campo vacío o fuera de rango no llega a Rust como 0: se conserva el valor anterior", async () => {
    const partida = await nuevoStore();
    (partida.rejilla as any).tamano = null;
    partida.marcarRejillaModificada();
    await partida.guardarRejilla();
    expect(backend.mapas.find((m) => m.id === 1)!.rejilla).toBe(50);
  });

  it("cambiar la imagen con rejilla pendiente no pisa ninguno de los dos cambios", async () => {
    const partida = await nuevoStore();
    partida.rejilla.tamano = 80;
    partida.marcarRejillaModificada();

    await partida.cambiarImagenMapa(partida.mapas.find((m) => m.id === 1)!, "/home/yo/mapa.png");

    const final = backend.mapas.find((m) => m.id === 1)!;
    expect(final.url_imagen_fondo).toBe("imagenes/mapa.png");
    expect(final.rejilla).toBe(80);
    expect(partida.mapaActual?.url_imagen_fondo).toBe("imagenes/mapa.png");
  });

  it("renombrar un mapa tampoco pierde la rejilla pendiente", async () => {
    const partida = await nuevoStore();
    partida.rejilla.grosor = 4;
    partida.marcarRejillaModificada();
    await partida.renombrarMapa(partida.mapas.find((m) => m.id === 1)!, "Taberna");
    const final = backend.mapas.find((m) => m.id === 1)!;
    expect([final.nombre, final.grosor]).toEqual(["Taberna", 4]);
  });

  it("la rejilla se guarda sola 5 s después del último cambio", async () => {
    const partida = await nuevoStore();
    partida.rejilla.tamano = 65;
    partida.marcarRejillaModificada();
    await vi.advanceTimersByTimeAsync(4_900);
    expect(llamadas("modificar_mapa")).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(200);
    expect(llamadas("modificar_mapa")).toHaveLength(1);
  });
});

describe("partidas", () => {
  it("cambiar guarda primero la rejilla pendiente y después deja solo lo de la otra partida", async () => {
    const partida = await nuevoStore();
    partida.rejilla.tamano = 120;
    partida.marcarRejillaModificada();
    partida.seleccionarInstancia(partida.instancias[0]);

    await partida.cambiarPartida("B");

    const orden = backend.llamadas.map((l) => l.cmd);
    expect(orden.indexOf("modificar_mapa")).toBeGreaterThan(-1);
    expect(orden.indexOf("modificar_mapa")).toBeLessThan(orden.indexOf("abrir_partida"));
    expect(backend.llamadas.find((l) => l.cmd === "modificar_mapa")!.args.mapa.rejilla).toBe(120);

    expect(partida.partidas.actual).toBe("B");
    expect(partida.zonas.map((z) => z.id)).toEqual([9]);
    expect(partida.mapaActual).toBeNull();
    expect(partida.mapas).toEqual([]);
    expect(partida.banco).toEqual([]);
    expect(partida.instancias).toEqual([]);
    expect(partida.instanciaSeleccionada).toBeNull();
    expect(partida.rejilla.tamano).toBe(50); // la de serie, no la del mapa de la otra partida
    // Sin timers colgados: la rejilla de la partida anterior no se escribe 5 s después en la nueva
    await vi.advanceTimersByTimeAsync(6_000);
    expect(llamadas("modificar_mapa")).toHaveLength(1);
  });

  it("crearPartida abre la nueva y recarga la lista", async () => {
    const partida = await nuevoStore();
    await partida.crearPartida("Campaña");
    expect(partida.partidas).toEqual({ actual: "Campaña", lista: ["A", "B", "Campaña"] });
    expect(partida.definiciones).toHaveLength(1); // se volvieron a leer
  });

  it("si Rust falla, el error llega a quien llama y la partida abierta sigue intacta", async () => {
    const partida = await nuevoStore();
    await expect(partida.crearPartida("B")).rejects.toMatch("Ya existe");
    await expect(partida.cambiarPartida("Z")).rejects.toMatch("no existe");
    expect(partida.zonas.map((z) => z.id)).toEqual([1, 2, 3]);
    expect(partida.mapaActual?.id).toBe(1);
    expect(partida.partidas.actual).toBe("A");
  });
});

describe("mapas y reordenación", () => {
  it("borrar el mapa abierto abre el siguiente; si era el último, el anterior", async () => {
    const partida = await nuevoStore();
    await partida.borrarMapa(partida.mapas.find((m) => m.id === 1)!);
    expect(partida.mapaActual?.id).toBe(2);
    await partida.borrarMapa(partida.mapas.find((m) => m.id === 2)!);
    expect(partida.mapaActual).toBeNull(); // la zona 1 se quedó sin mapas
  });

  it("borrar un mapa que no es el abierto no cambia el abierto", async () => {
    const partida = await nuevoStore();
    await partida.borrarMapa(partida.mapas.find((m) => m.id === 2)!);
    expect(partida.mapaActual?.id).toBe(1);
  });

  it("crear un mapa lo abre", async () => {
    const partida = await nuevoStore();
    await partida.crearMapa("/img/fondo.png");
    expect(partida.mapaActual?.url_imagen_fondo).toBe("imagenes/fondo.png");
    expect(partida.mapaActual?.id_zona).toBe(1);
  });

  it("reordenar zonas solo escribe las que cambian de posición", async () => {
    const partida = await nuevoStore();
    await partida.reordenarZonas([3, 1, 2]);
    expect(partida.zonasOrdenadas.map((z) => z.id)).toEqual([3, 1, 2]);
    expect(llamadas("modificar_zona")).toHaveLength(3);

    backend.llamadas.length = 0;
    await partida.reordenarZonas([3, 1, 2]); // sin cambios
    expect(llamadas("modificar_zona")).toHaveLength(0);
  });
});

describe("fichas: selección y vida", () => {
  it("seleccionar una ficha la expone como instanciaSeleccionada", async () => {
    const partida = await nuevoStore();
    partida.seleccionarInstancia(partida.instancias[1]);
    expect(partida.idsSeleccionados).toEqual([2]);
    expect(partida.instanciaSeleccionada?.base.nombre).toBe("Lobo");
    partida.seleccionarInstancia(null);
    expect(partida.instanciaSeleccionada).toBeNull();
  });

  it("con Shift se añaden o quitan fichas del foco; con varias no hay «una seleccionada»", async () => {
    const partida = await nuevoStore();
    const [a, b] = partida.instancias;
    partida.seleccionarInstancia(a);
    partida.seleccionarInstancia(b, true);
    expect(partida.idsSeleccionados).toEqual([1, 2]);
    expect(partida.instanciaSeleccionada).toBeNull();
    expect([partida.estaSeleccionada(1), partida.estaSeleccionada(2)]).toEqual([true, true]);

    partida.seleccionarInstancia(a, true); // otra vez con Shift: la quita
    expect(partida.idsSeleccionados).toEqual([2]);
    partida.seleccionarInstancia(a); // sin Shift: solo esa
    expect(partida.idsSeleccionados).toEqual([1]);
  });

  it("↑/↓ pasan a la ficha anterior/siguiente de «Fichas activas», solo con una con foco", async () => {
    const partida = await nuevoStore();
    await partida.reordenarInstancias([2, 1]);
    expect(partida.instanciasOrdenadas.map((i) => i.id_instancia)).toEqual([2, 1]);

    partida.seleccionarInstancia(partida.instancias.find((i) => i.id_instancia === 2)!);
    expect(partida.seleccionarVecina(-1)).toBe(false); // ya es la primera
    expect(partida.seleccionarVecina(1)).toBe(true);
    expect(partida.idsSeleccionados).toEqual([1]);

    partida.seleccionarInstancia(partida.instancias[1], true); // dos con foco
    expect(partida.seleccionarVecina(-1)).toBe(false);
  });

  it("reordenar y rotar llaman a Rust", async () => {
    const partida = await nuevoStore();
    await partida.reordenarInstancias([2, 1]);
    expect(llamadas("reordenar_instancias")[0].args).toEqual({ ids: [2, 1] });
    await partida.rotarInstancia(1);
    expect(partida.instancias[0].rotacion).toBe(1);
  });

  it("si Rust elimina la ficha seleccionada (al borrar su zona), la selección desaparece sola", async () => {
    const partida = await nuevoStore();
    partida.seleccionarInstancia(partida.instancias[0]); // ficha del mapa 1, zona 1
    await partida.borrarZona(partida.zonas.find((z) => z.id === 1)!);
    expect(partida.instanciaSeleccionada).toBeNull();
  });

  it("50 pasos de rueda se ven al momento y se mandan como UN cambio de 50 al pasar 1 s", async () => {
    const partida = await nuevoStore();
    const orco = () => partida.instancias[0]; // vida 10 de 18
    partida.seleccionarInstancia(orco());

    for (let i = 0; i < 6; i++) partida.cambiarValorActual(1, 1, 1);
    expect(partida.valorMostrado(orco(), 1)).toBe(16);
    for (let i = 0; i < 50; i++) partida.cambiarValorActual(1, 1, -1);
    expect(partida.valorMostrado(orco(), 1)).toBe(0); // nunca baja de 0
    expect(llamadas("modificar_valor_actual")).toHaveLength(0); // aún nada

    await vi.advanceTimersByTimeAsync(999);
    expect(llamadas("modificar_valor_actual")).toHaveLength(0);
    partida.cambiarValorActual(1, 1, 3); // otro paso reinicia la espera
    await vi.advanceTimersByTimeAsync(1000);
    expect(llamadas("modificar_valor_actual").map((l) => l.args)).toEqual([{ idInstancia: 1, idDefinicion: 1, delta: -7 }]);
    expect(partida.valorMostrado(orco(), 1)).toBe(3);
    expect(partida.valoresPendientes).toEqual({});
  });

  it("los cambios pendientes no pasan del máximo y se mandan ya si la ficha pierde el foco", async () => {
    const partida = await nuevoStore();
    partida.seleccionarInstancia(partida.instancias[0]);
    for (let i = 0; i < 20; i++) partida.cambiarValorActual(1, 1, 1);
    expect(partida.valorMostrado(partida.instancias[0], 1)).toBe(18); // el máximo

    partida.seleccionarInstancia(null);
    await vi.advanceTimersByTimeAsync(0);
    expect(llamadas("modificar_valor_actual").map((l) => l.args.delta)).toEqual([8]);
  });

  it("instanciar, mover, quitar del tablero y borrar llaman a Rust y vuelven a leer las instancias", async () => {
    const partida = await nuevoStore();
    const destino = { id_mapa: 1, casilla: { x: 3, y: 4 } };

    await partida.instanciar(1, destino);
    expect(llamadas("instanciar_entidad")[0].args).toEqual({ idEntidad: 1, destino });
    expect(partida.instancias).toHaveLength(3);
    expect(partida.idsSeleccionados).toEqual([3]); // la nueva queda seleccionada

    await partida.instanciar(2, null); // solo activa
    expect(partida.instancias[3].coordenadas).toBe("NoPuesto");

    await partida.moverInstancia(4, { id_mapa: 1, casilla: { x: 0, y: 0 } });
    expect(partida.instancias[3].coordenadas).toEqual({ Casilla: { x: 0, y: 0 } });

    await partida.quitarDelTablero(4);
    expect(llamadas("mover_instancia")[1].args).toEqual({ idInstancia: 4, destino: null });
    expect(partida.instancias[3].id_mapa).toBeNull();

    partida.seleccionarInstancia(partida.instancias[3]);
    await partida.borrarInstancia(4);
    expect(partida.instancias.map((i) => i.id_instancia)).toEqual([1, 2, 3]);
    expect(partida.idsSeleccionados).toEqual([]);
  });

  it("si Rust rechaza el cambio, no rompe nada y las instancias se releen igual", async () => {
    const partida = await nuevoStore();
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    await partida.moverInstancia(99, { id_mapa: 1, casilla: { x: 0, y: 0 } });
    expect(error).toHaveBeenCalled();
    expect(partida.instancias).toHaveLength(2);
    error.mockRestore();
  });

  it("carga las definiciones de los parámetros al arrancar", async () => {
    const partida = await nuevoStore();
    expect(partida.definiciones.map((d) => d.clave)).toEqual(["Vida"]);
  });

  describe("parámetros de la partida", () => {
    const nueva = (clave: string) => ({
      clave, valor_por_defecto: { Numero: 3 }, tiene_maximo: false, representar_mapa: null,
      visibilidad: "Variable" as const, unidad: null
    });

    it("crear, reordenar y borrar recargan las definiciones y las fichas", async () => {
      const partida = await nuevoStore();
      await partida.crearDefinicion(nueva("Cordura"));
      await partida.crearDefinicion(nueva("Maná"));
      expect(partida.definiciones.map((d) => d.clave)).toEqual(["Vida", "Cordura", "Maná"]);

      await partida.reordenarDefiniciones([3, 1, 2]);
      expect(partida.definiciones.map((d) => d.clave)).toEqual(["Maná", "Vida", "Cordura"]);

      const antes = llamadas("obtener_banco_entidades").length;
      await partida.borrarDefinicion(1);
      expect(partida.definiciones.map((d) => d.clave)).toEqual(["Maná", "Cordura"]);
      expect(llamadas("obtener_banco_entidades").length).toBe(antes + 1); // Rust cambió las fichas
    });

    it("modificarDefinicion manda la definición con su id y recarga", async () => {
      const partida = await nuevoStore();
      await partida.modificarDefinicion({ ...partida.definiciones[0], clave: "Puntos de vida", unidad: "pv" });
      expect(partida.definiciones[0]).toMatchObject({ id: 1, clave: "Puntos de vida", unidad: "pv" });
      expect(llamadas("modificar_definicion_parametro")[0].args.definicion.id).toBe(1);
    });

    it("si Rust rechaza el cambio, el error llega a quien llama y todo se vuelve a leer", async () => {
      const partida = await nuevoStore();
      await expect(partida.crearDefinicion(nueva("Vida"))).rejects.toMatch("Ya existe");
      expect(partida.definiciones).toHaveLength(1);
    });
  });
});
