// Pruebas del formulario de creación de parámetros (src/lib/formularioParametro.svelte.ts).
import { beforeEach, describe, expect, it, vi } from "vitest";

let FormularioParametro: typeof import("$lib/formularioParametro.svelte").FormularioParametro;
let resumenDefinicion: typeof import("$lib/formularioParametro.svelte").resumenDefinicion;

beforeEach(async () => {
  vi.resetModules();
  vi.doMock("@tauri-apps/api/core", () => ({ invoke: async () => undefined, convertFileSrc: (p: string) => p }));
  ({ FormularioParametro, resumenDefinicion } = await import("$lib/formularioParametro.svelte"));
});

describe("FormularioParametro", () => {
  it("convierte lo escrito en una definición sin id", () => {
    const f = new FormularioParametro();
    f.clave = "  Cordura ";
    f.valorNumero = 8;
    f.tieneMaximo = true;
    f.unidad = " pts ";
    f.visibilidad = "SiemprePrivado";
    f.forma = "Circulo";
    f.color = "#ff0000";
    expect(f.aDefinicion()).toEqual({
      clave: "Cordura",
      valor_por_defecto: { Numero: 8 },
      tiene_maximo: true,
      representar_mapa: { Circulo: [255, 0, 0, 90] },
      visibilidad: "SiemprePrivado",
      unidad: "pts"
    });
  });

  it("un texto ignora máximo, unidad y forma aunque se hubieran marcado antes", () => {
    const f = new FormularioParametro();
    f.clave = "Apodo";
    f.tieneMaximo = true;
    f.unidad = "m";
    f.forma = "Rombo";
    f.tipo = "Texto";
    f.valorTexto = "Sin nombre";
    expect(f.aDefinicion()).toMatchObject({
      valor_por_defecto: { Texto: "Sin nombre" },
      tiene_maximo: false,
      representar_mapa: null,
      unidad: null
    });
  });

  it("rechaza el nombre vacío y los valores que no son enteros entre 0 y 32767", () => {
    const f = new FormularioParametro();
    expect(() => f.aDefinicion()).toThrow("necesita un nombre");
    f.clave = "X";
    for (const malo of [-1, 1.5, 40000, "" as any, null as any]) {
      f.valorNumero = malo;
      expect(() => f.aDefinicion(), String(malo)).toThrow();
    }
    f.valorNumero = 0;
    expect(() => f.aDefinicion()).not.toThrow();
  });

  it("cargar y aDefinicion son inversos, y el id queda para saber que se edita", () => {
    const d = {
      id: 3, clave: "Velocidad", valor_por_defecto: { Numero: 6 }, tiene_maximo: false,
      representar_mapa: { Rombo: [59, 130, 246, 90] }, visibilidad: "SiemprePublico" as const, unidad: "m"
    };
    const f = new FormularioParametro();
    f.cargar(d);
    expect(f.idEditando).toBe(3);
    const { ...sinId } = d;
    delete (sinId as any).id;
    expect(f.aDefinicion()).toEqual(sinId);
    f.reiniciar();
    expect(f.idEditando).toBeNull();
  });

  it("reiniciar deja el formulario como nuevo", () => {
    const f = new FormularioParametro();
    f.clave = "X";
    f.tipo = "Texto";
    f.forma = "Cuadrado";
    f.reiniciar();
    expect([f.clave, f.tipo, f.forma, f.visibilidad]).toEqual(["", "Numero", "ninguna", "Variable"]);
  });
});

describe("resumenDefinicion", () => {
  it("describe el tipo, el máximo, la unidad, la forma y la visibilidad", () => {
    expect(
      resumenDefinicion({
        id: 3, clave: "Velocidad", valor_por_defecto: { Numero: 6 }, tiene_maximo: false,
        representar_mapa: { Rombo: [1, 2, 3, 4] }, visibilidad: "SiemprePublico", unidad: "m"
      })
    ).toBe("Número · m · Rombo · siempre público");
    expect(
      resumenDefinicion({
        id: 1, clave: "Vida", valor_por_defecto: { Numero: 10 }, tiene_maximo: true,
        representar_mapa: null, visibilidad: "Variable", unidad: null
      })
    ).toBe("Número · con máximo · cada ficha decide");
  });
});
