// El color de las marcas de cada persona: funciones puras (src/lib/colorMarca.ts), su preferencia guardada
// (ajustesVista) y las flechas de los demás, una por persona (src/lib/recorridosAjenos.ts).
import { beforeEach, describe, expect, it, vi } from "vitest";
import { COLOR_VERDE, aHex, canal, colorAleatorio, deHex, esColor, hslARgb, limitarColor, rgbCss } from "$lib/colorMarca";
import { RecorridosAjenos } from "$lib/recorridosAjenos";
import type { AvisoTablero } from "$lib/avisosTablero";

describe("colores", () => {
  it("cada canal se queda entero entre 0 y 255", () => {
    expect([canal(-5), canal(300), canal(12.6), canal(NaN), canal(Infinity)]).toEqual([0, 255, 13, 0, 0]);
    expect(limitarColor([-1, 128.4, 999])).toEqual([0, 128, 255]);
  });

  it("de HSL a RGB (valores conocidos)", () => {
    expect(hslARgb(0, 1, 0.5)).toEqual([255, 0, 0]);
    expect(hslARgb(120, 1, 0.5)).toEqual([0, 255, 0]);
    expect(hslARgb(240, 1, 0.5)).toEqual([0, 0, 255]);
    expect(hslARgb(0, 0, 1)).toEqual([255, 255, 255]);
  });

  it("el color aleatorio siempre es válido y vivo (ni gris, ni casi negro, ni casi blanco)", () => {
    for (let i = 0; i < 200; i++) {
      const c = colorAleatorio();
      expect(esColor(c)).toBe(true);
      const [mayor, menor] = [Math.max(...c), Math.min(...c)];
      expect(mayor - menor, `${c}`).toBeGreaterThan(80); // saturado
      expect(mayor, `${c}`).toBeGreaterThan(120); // no oscuro
    }
    // Con el azar fijo sale siempre lo mismo (y los tonos recorren todo el círculo)
    expect(colorAleatorio(() => 0)).toEqual(colorAleatorio(() => 0));
    expect(colorAleatorio(() => 0)).not.toEqual(colorAleatorio(() => 0.5));
  });

  it("hex ↔ color, con y sin #, de 3 o 6 cifras, y lo que no es un color da null", () => {
    expect(aHex([0, 128, 255])).toBe("#0080ff");
    expect(aHex([300, -2, 5])).toBe("#ff0005");
    expect(deHex("#0080FF")).toEqual([0, 128, 255]);
    expect(deHex(" 0080ff ")).toEqual([0, 128, 255]);
    expect(deHex("#f80")).toEqual([255, 136, 0]);
    for (const mal of ["", "#12", "#12345", "#1234567", "#gggggg", "rojo"]) expect(deHex(mal), mal).toBeNull();
    for (let i = 0; i < 50; i++) {
      const c = colorAleatorio();
      expect(deHex(aHex(c))).toEqual(c);
    }
  });

  it("rgb para el canvas, con o sin transparencia; y solo se aceptan tres enteros de 0 a 255", () => {
    expect(rgbCss([1, 2, 3])).toBe("rgb(1, 2, 3)");
    expect(rgbCss([1, 2, 3], 0.5)).toBe("rgba(1, 2, 3, 0.5)");
    expect(rgbCss([999, 2, 3])).toBe("rgb(255, 2, 3)");
    expect(esColor([0, 255, 7])).toBe(true);
    for (const mal of [null, [1, 2], [1, 2, 3, 4], [1, 2, 256], [1, 2, -1], [1, 2, 1.5], ["1", 2, 3], "rojo"]) expect(esColor(mal)).toBe(false);
  });
});

describe("mi color de flecha (preferencia de pantalla)", () => {
  const guardado: Record<string, string> = {};
  beforeEach(() => {
    for (const k of Object.keys(guardado)) delete guardado[k];
    vi.resetModules();
    vi.stubGlobal("localStorage", {
      getItem: (k: string) => guardado[k] ?? null,
      setItem: (k: string, v: string) => void (guardado[k] = v),
      removeItem: (k: string) => void delete guardado[k]
    });
    vi.doMock("@tauri-apps/api/core", () => ({ invoke: async () => null, convertFileSrc: (p: string) => p }));
    vi.doMock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));
  });

  const cargar = async () => (await import("$lib/ajustesVista.svelte")).ajustesVista;

  it("la primera vez se sortea y se guarda, y la siguiente se recuerda el mismo", async () => {
    const a = await cargar();
    expect(esColor(a.colorFlecha)).toBe(true);
    expect(JSON.parse(guardado["openroll.ajustesVista"]).colorFlecha).toEqual(a.colorFlecha);
    vi.resetModules();
    expect((await cargar()).colorFlecha).toEqual(a.colorFlecha);
  });

  it("se puede cambiar (limitado a 0–255) y sortear otro, y se recuerda", async () => {
    const a = await cargar();
    a.cambiarColorFlecha([300, -4, 99.4]);
    expect(a.colorFlecha).toEqual([255, 0, 99]);
    vi.resetModules();
    expect((await cargar()).colorFlecha).toEqual([255, 0, 99]);
    const b = await cargar();
    const antes = [...b.colorFlecha];
    // Un sorteo puede repetir, pero en varios intentos sale otro
    const vistos = new Set<string>([antes.join()]);
    for (let i = 0; i < 20; i++) {
      b.colorFlechaAleatorio();
      expect(esColor(b.colorFlecha)).toBe(true);
      vistos.add(b.colorFlecha.join());
    }
    expect(vistos.size).toBeGreaterThan(1);
  });

  it("lo guardado roto (o con un canal fuera de rango) se descarta y se sortea otro", async () => {
    guardado["openroll.ajustesVista"] = JSON.stringify({ colorFlecha: [1, 2, 999], tutoriales: false });
    const a = await cargar();
    expect(esColor(a.colorFlecha)).toBe(true);
    expect(a.tutoriales).toBe(false); // lo demás se conserva
    expect(esColor(JSON.parse(guardado["openroll.ajustesVista"]).colorFlecha)).toBe(true);
  });

  it("sin almacenamiento (ventana privada) no falla: el color dura la sesión", async () => {
    vi.stubGlobal("localStorage", {
      getItem: () => {
        throw new Error("no");
      },
      setItem: () => {
        throw new Error("no");
      }
    });
    const a = await cargar();
    expect(esColor(a.colorFlecha)).toBe(true);
    expect(() => a.cambiarColorFlecha([1, 2, 3])).not.toThrow();
    expect(a.colorFlecha).toEqual([1, 2, 3]);
  });
});

describe("las flechas de los demás: una por persona, todas a la vez", () => {
  const flecha = (jugador: number | undefined, n: number, color?: [number, number, number], id_mapa = 1): AvisoTablero => ({
    Recorrido: { id_mapa, casillas: Array.from({ length: n }, (_, i) => ({ x: i, y: 0 })), color, jugador }
  });
  const borrar = (jugador: number | undefined, id_mapa = 1): AvisoTablero => ({ RecorridoBorrado: { id_mapa, jugador } });

  it("dos personas dibujan a la vez y se ven las dos, cada una con su color", () => {
    const r = new RecorridosAjenos();
    expect(r.recibir(flecha(2, 3, [255, 0, 0]), 1)).toBe(true);
    expect(r.recibir(flecha(0, 5, [0, 0, 255]), 1)).toBe(true); // el host (jugador 0)
    expect(r.lista().map((f) => [f.jugador, f.casillas.length, f.color])).toEqual([
      [0, 5, [0, 0, 255]],
      [2, 3, [255, 0, 0]]
    ]);
    // Una persona cambia su flecha: se sustituye solo la suya
    r.recibir(flecha(2, 7, [255, 0, 0]), 1);
    expect(r.lista().find((f) => f.jugador === 2)!.casillas).toHaveLength(7);
    expect(r.lista().find((f) => f.jugador === 0)!.casillas).toHaveLength(5);
  });

  it("quitar la flecha de uno no toca la de los demás", () => {
    const r = new RecorridosAjenos();
    r.recibir(flecha(1, 2), 1);
    r.recibir(flecha(2, 2), 1);
    expect(r.recibir(borrar(1), 1)).toBe(true);
    expect(r.lista().map((f) => f.jugador)).toEqual([2]);
    expect(r.recibir(borrar(1), 1)).toBe(false); // ya no estaba
    r.quitar(2);
    expect(r.lista()).toEqual([]);
  });

  it("un aviso sin `jugador` es del host y sin color o con uno roto es verde; el de otro mapa o de otro tipo se ignora", () => {
    const r = new RecorridosAjenos();
    r.recibir(flecha(undefined, 2), 1);
    r.recibir({ Recorrido: { id_mapa: 1, casillas: [{ x: 0, y: 0 }], color: [1, 2, 999] as any, jugador: 3 } }, 1);
    expect(r.lista().map((f) => [f.jugador, f.color])).toEqual([
      [0, COLOR_VERDE],
      [3, COLOR_VERDE]
    ]);
    expect(r.recibir(flecha(4, 2, undefined, 9), 1)).toBe(false); // de otro mapa
    expect(r.recibir(borrar(0, 9), 1)).toBe(false);
    expect(r.recibir({ Ping: { id_mapa: 1, casilla: { x: 0, y: 0 } } }, 1)).toBe(false);
    expect(r.lista()).toHaveLength(2);
  });

  it("limpiar quita todas (al cambiar de mapa)", () => {
    const r = new RecorridosAjenos();
    r.recibir(flecha(1, 2), 1);
    r.recibir(flecha(2, 2), 1);
    r.limpiar();
    expect(r.lista()).toEqual([]);
  });
});
