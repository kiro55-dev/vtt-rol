// Pruebas del banco en otra ventana (src/lib/bancoExterno.svelte.ts): la conversión de posiciones
// y el estado de la ventana, con las ventanas de Tauri falsas.
import { beforeEach, describe, expect, it, vi } from "vitest";

const destruida: Array<() => void> = [];
let creadas = 0;
let existente: any = null;

class VentanaFalsa {
  static getByLabel = vi.fn(async () => existente);
  constructor(public etiqueta: string, public opciones: any) {
    creadas++;
  }
  once = vi.fn(async (evento: string, cb: () => void) => {
    if (evento === "tauri://destroyed") destruida.push(cb);
    return () => {};
  });
  setFocus = vi.fn(async () => {});
  close = vi.fn(async () => {});
}

let lib: typeof import("$lib/bancoExterno.svelte");

beforeEach(async () => {
  vi.resetModules();
  destruida.length = 0;
  creadas = 0;
  existente = null;
  vi.doMock("@tauri-apps/api/core", () => ({ invoke: async () => [], convertFileSrc: (p: string) => p }));
  vi.doMock("@tauri-apps/api/event", () => ({ emit: vi.fn(async () => {}), listen: vi.fn(async () => () => {}) }));
  vi.doMock("@tauri-apps/api/webviewWindow", () => ({ WebviewWindow: VentanaFalsa }));
  vi.doMock("@tauri-apps/api/window", () => ({
    cursorPosition: async () => ({ x: 0, y: 0 }),
    getCurrentWindow: () => ({ onCloseRequested: async () => () => {}, setFocus: async () => {} })
  }));
  lib = await import("$lib/bancoExterno.svelte");
});

describe("aVentana", () => {
  it("pasa la posición del cursor en la pantalla a px de la ventana", () => {
    // Ventana en (100, 50) físicos, sin escala
    expect(lib.aVentana({ x: 300, y: 250 }, { x: 100, y: 50, escala: 1 })).toEqual({ x: 200, y: 200 });
    // Con escala 1.5 los px del navegador son los físicos entre 1.5
    expect(lib.aVentana({ x: 400, y: 200 }, { x: 100, y: 50, escala: 1.5 })).toEqual({ x: 200, y: 100 });
    expect(lib.aVentana({ x: 300, y: 250 }, { x: 100, y: 50, escala: 2 })).toEqual({ x: 100, y: 100 });
  });

  it("vale con una ventana en un monitor a la izquierda (posición negativa) y fuera de ella", () => {
    expect(lib.aVentana({ x: -1500, y: 100 }, { x: -1920, y: 0, escala: 1 })).toEqual({ x: 420, y: 100 });
    // Cursor a la izquierda de la ventana: sale negativo, y no se recorta (la zona no será ninguna)
    expect(lib.aVentana({ x: 50, y: 60 }, { x: 100, y: 50, escala: 1 })).toEqual({ x: -50, y: 10 });
  });
});

describe("la ventana del banco", () => {
  it("abrir crea una sola ventana, y si se destruye el banco vuelve a la principal", async () => {
    expect(lib.bancoExterno.abierto).toBe(false);
    await lib.bancoExterno.abrir();
    expect(lib.bancoExterno.abierto).toBe(true);
    expect(creadas).toBe(1);

    await lib.bancoExterno.abrir(); // ya abierta: no crea otra
    expect(creadas).toBe(1);

    destruida.forEach((cb) => cb()); // el usuario la cierra con la X
    expect(lib.bancoExterno.abierto).toBe(false);
  });

  it("si ya existía una ventana del banco (recarga en desarrollo), la reutiliza", async () => {
    existente = new VentanaFalsa("banco", {});
    creadas = 0;
    await lib.bancoExterno.abrir();
    expect(creadas).toBe(0);
    expect(lib.bancoExterno.abierto).toBe(true);
    expect(existente.setFocus).toHaveBeenCalled();
  });

  it("cerrar cierra la ventana y deja el banco en la principal", async () => {
    existente = new VentanaFalsa("banco", {});
    await lib.bancoExterno.abrir();
    await lib.bancoExterno.cerrar();
    expect(existente.close).toHaveBeenCalled();
    expect(lib.bancoExterno.abierto).toBe(false);
  });
});
