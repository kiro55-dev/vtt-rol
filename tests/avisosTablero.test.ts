// Pruebas de los avisos efímeros del tablero (src/lib/avisosTablero.ts).
import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn(async (_cmd: string, _args?: any) => undefined);
let escuchar: (e: { payload: unknown }) => void = () => {};
const quitarEscucha = vi.fn();
let lib: typeof import("$lib/avisosTablero");

beforeEach(async () => {
  vi.resetModules();
  invoke.mockReset().mockResolvedValue(undefined);
  quitarEscucha.mockClear();
  vi.doMock("@tauri-apps/api/core", () => ({ invoke }));
  vi.doMock("@tauri-apps/api/event", () => ({
    listen: vi.fn(async (_nombre: string, cb: typeof escuchar) => {
      escuchar = cb;
      return quitarEscucha;
    })
  }));
  lib = await import("$lib/avisosTablero");
});

describe("avisos del tablero", () => {
  it("el formato es el que serializa Rust", () => {
    // Mismos JSON que comprueba el test de tablero.rs
    expect(lib.avisoPing(3, { x: 4, y: 5 }, [1, 2, 3])).toEqual({ Ping: { id_mapa: 3, casilla: { x: 4, y: 5 }, color: [1, 2, 3] } });
    expect(lib.avisoRecorrido(3, [{ x: 0, y: 0 }], [9, 8, 7])).toEqual({ Recorrido: { id_mapa: 3, casillas: [{ x: 0, y: 0 }], color: [9, 8, 7] } });
    expect(lib.avisoRecorridoBorrado(3)).toEqual({ RecorridoBorrado: { id_mapa: 3 } });
    expect(lib.avisoFoco([4, 7])).toEqual({ Foco: { ids_instancias: [4, 7] } });
  });

  it("el recorrido enviado es una copia: seguir moviendo el ratón no lo cambia", () => {
    const casillas = [{ x: 0, y: 0 }];
    const aviso = lib.avisoRecorrido(1, casillas, [0, 0, 0]) as any;
    casillas.push({ x: 1, y: 0 });
    expect(aviso.Recorrido.casillas).toHaveLength(1);
  });

  it("enviar llama al comando de Rust con el aviso", () => {
    const aviso = lib.avisoPing(2, { x: 1, y: 1 }, [0, 0, 0]);
    lib.enviarAviso(aviso);
    expect(invoke).toHaveBeenCalledWith("avisar_tablero", { aviso });
  });

  it("un aviso que Rust rechaza no rompe nada", async () => {
    const aviso = vi.spyOn(console, "warn").mockImplementation(() => {});
    invoke.mockRejectedValue("Ese mapa no existe");
    expect(() => lib.enviarAviso(lib.avisoRecorridoBorrado(9))).not.toThrow();
    await new Promise((r) => setTimeout(r, 0));
    expect(aviso).toHaveBeenCalled();
    aviso.mockRestore();
  });

  it("recibir entrega el contenido del evento y se puede dejar de escuchar", async () => {
    const recibidos: unknown[] = [];
    const dejar = lib.alRecibirAviso((a) => recibidos.push(a));
    await new Promise((r) => setTimeout(r, 0));

    const ping = lib.avisoPing(1, { x: 2, y: 3 }, [0, 0, 0]);
    escuchar({ payload: ping });
    expect(recibidos).toEqual([ping]);

    dejar();
    expect(quitarEscucha).toHaveBeenCalledOnce();
  });
});
