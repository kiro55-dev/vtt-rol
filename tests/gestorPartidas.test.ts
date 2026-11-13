// Pruebas de la lógica de la ventana de partidas (src/lib/gestorPartidas.svelte.ts) con stores falsos.
import { beforeEach, describe, expect, it, vi } from "vitest";

let partidaFalsa: any;
let tiradasFalsas: any;
let GestorPartidas: typeof import("$lib/gestorPartidas.svelte").GestorPartidas;

beforeEach(async () => {
  vi.resetModules();
  partidaFalsa = {
    partidas: { actual: "A", lista: ["A", "B"] },
    recargarPartidas: vi.fn(async () => {}),
    cambiarPartida: vi.fn(async () => {}),
    crearPartida: vi.fn(async () => {}),
    borrarPartida: vi.fn(async () => {}),
    renombrarPartida: vi.fn(async () => {})
  };
  tiradasFalsas = { cargar: vi.fn(async () => {}) };
  vi.doMock("$lib/partida.svelte", () => ({ partida: partidaFalsa }));
  vi.doMock("$lib/tiradas.svelte", () => ({ tiradas: tiradasFalsas }));
  ({ GestorPartidas } = await import("$lib/gestorPartidas.svelte"));
});

describe("GestorPartidas: renombrar", () => {
  it("empezar a renombrar pone el nombre actual en el campo", () => {
    const g = new GestorPartidas();
    g.empezarRenombrar("B");
    expect([g.renombrando, g.textoNuevo]).toEqual(["B", "B"]);
  });

  it("guardar cambia el nombre y cierra el campo; recarga las tiradas (son de cada partida)", async () => {
    const g = new GestorPartidas();
    g.empezarRenombrar("B");
    g.textoNuevo = "  Campaña  ";
    expect(await g.guardarRenombrado()).toBe(true);
    expect(partidaFalsa.renombrarPartida).toHaveBeenCalledWith("B", "Campaña");
    expect(g.renombrando).toBeNull();
    expect(tiradasFalsas.cargar).toHaveBeenCalledOnce();
    expect(g.error).toBe("");
  });

  it("vacío avisa y no pregunta a Rust; el mismo nombre solo cierra el campo; cancelar lo cierra sin cambiar nada", async () => {
    const g = new GestorPartidas();
    g.empezarRenombrar("B");
    g.textoNuevo = "   ";
    expect(await g.guardarRenombrado()).toBe(false);
    expect(g.error).toContain("necesita un nombre");
    expect(g.renombrando).toBe("B"); // sigue editando

    g.textoNuevo = "B";
    expect(await g.guardarRenombrado()).toBe(true);
    expect(g.renombrando).toBeNull();
    expect(partidaFalsa.renombrarPartida).not.toHaveBeenCalled();

    g.empezarRenombrar("B");
    g.cancelarRenombrar();
    expect([g.renombrando, g.error]).toEqual([null, ""]);
  });

  it("si Rust lo rechaza (repetido, sala abierta…) se ve el motivo y el campo sigue abierto para corregirlo", async () => {
    partidaFalsa.renombrarPartida = vi.fn(async () => {
      throw "Ya existe una partida llamada «A»";
    });
    const g = new GestorPartidas();
    g.empezarRenombrar("B");
    g.textoNuevo = "A";
    expect(await g.guardarRenombrado()).toBe(false);
    expect(g.error).toContain("Ya existe");
    expect(g.renombrando).toBe("B");
    expect(g.ocupado).toBe(false);
  });
});

describe("GestorPartidas: borrar", () => {
  it("borra otra partida sin cambiar de partida", async () => {
    const g = new GestorPartidas();
    expect(await g.borrar("B")).toBe(true);
    expect(partidaFalsa.borrarPartida).toHaveBeenCalledWith("B");
    expect(partidaFalsa.cambiarPartida).not.toHaveBeenCalled();
    expect([g.ocupado, g.error]).toEqual([false, ""]);
  });

  it("también se puede borrar la abierta: se le pide a Rust y se recargan las tiradas (la que queda es otra partida)", async () => {
    const g = new GestorPartidas();
    expect(await g.borrar("A")).toBe(true);
    expect(partidaFalsa.borrarPartida).toHaveBeenCalledWith("A");
    expect(tiradasFalsas.cargar).toHaveBeenCalledOnce();
    expect(g.error).toBe("");
  });

  it("si Rust lo rechaza, se ve el motivo; y no admite dos borrados a la vez", async () => {
    partidaFalsa.borrarPartida = vi.fn(async () => {
      throw "No se pudo borrar la partida: acceso denegado";
    });
    const g = new GestorPartidas();
    expect(await g.borrar("B")).toBe(false);
    expect(g.error).toContain("acceso denegado");
    expect(g.ocupado).toBe(false);

    let terminar!: () => void;
    partidaFalsa.borrarPartida = vi.fn(() => new Promise<void>((r) => (terminar = r)));
    const primero = g.borrar("B");
    expect(g.ocupado).toBe(true);
    expect(await g.borrar("B")).toBe(false);
    expect(partidaFalsa.borrarPartida).toHaveBeenCalledOnce();
    terminar();
    expect(await primero).toBe(true);
  });
});

describe("GestorPartidas", () => {
  it("abrir otra partida la abre y recarga las tiradas; la que ya está abierta no hace nada", async () => {
    const g = new GestorPartidas();
    expect(await g.abrir("A")).toBe(false);
    expect(partidaFalsa.cambiarPartida).not.toHaveBeenCalled();

    expect(await g.abrir("B")).toBe(true);
    expect(partidaFalsa.cambiarPartida).toHaveBeenCalledWith("B");
    expect(tiradasFalsas.cargar).toHaveBeenCalledOnce();
    expect([g.ocupado, g.error]).toEqual([false, ""]);
  });

  it("mientras cambia está ocupado y no admite otro cambio a la vez", async () => {
    let terminar!: () => void;
    partidaFalsa.cambiarPartida = vi.fn(() => new Promise<void>((r) => (terminar = r)));
    const g = new GestorPartidas();

    const primero = g.abrir("B");
    expect(g.ocupado).toBe(true);
    expect(await g.abrir("B")).toBe(false);
    expect(partidaFalsa.cambiarPartida).toHaveBeenCalledOnce();

    terminar();
    expect(await primero).toBe(true);
    expect(g.ocupado).toBe(false);
  });

  it("si Rust falla muestra el motivo y no recarga las tiradas", async () => {
    partidaFalsa.cambiarPartida = vi.fn(async () => {
      throw "No se pudo guardar la partida actual: disco lleno";
    });
    const g = new GestorPartidas();
    expect(await g.abrir("B")).toBe(false);
    expect(g.error).toContain("disco lleno");
    expect(g.ocupado).toBe(false);
    expect(tiradasFalsas.cargar).not.toHaveBeenCalled();

    // El siguiente intento limpia el error
    partidaFalsa.cambiarPartida = vi.fn(async () => {});
    await g.abrir("B");
    expect(g.error).toBe("");
  });

  it("crear usa el nombre sin espacios, vacía el campo al acabar y rechaza el vacío sin llamar a Rust", async () => {
    const g = new GestorPartidas();
    g.nombreNuevo = "   ";
    expect(await g.crear()).toBe(false);
    expect(g.error).toBe("La partida necesita un nombre");
    expect(partidaFalsa.crearPartida).not.toHaveBeenCalled();

    g.nombreNuevo = "  Mi campaña ";
    expect(await g.crear()).toBe(true);
    expect(partidaFalsa.crearPartida).toHaveBeenCalledWith("Mi campaña");
    expect(g.nombreNuevo).toBe("");
    expect(g.error).toBe("");
  });

  it("si crear falla (nombre repetido) conserva lo escrito para corregirlo", async () => {
    partidaFalsa.crearPartida = vi.fn(async () => {
      throw "Ya existe una partida llamada «B»";
    });
    const g = new GestorPartidas();
    g.nombreNuevo = "B";
    expect(await g.crear()).toBe(false);
    expect(g.nombreNuevo).toBe("B");
    expect(g.error).toContain("Ya existe");
  });
});
