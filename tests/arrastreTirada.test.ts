// Pruebas de arrastrar una tirada sobre un valor (src/lib/arrastreTirada.svelte.ts).
import { beforeEach, describe, expect, it, vi } from "vitest";

let lib: typeof import("$lib/arrastreTirada.svelte");

beforeEach(async () => {
  vi.resetModules();
  vi.doMock("@tauri-apps/api/core", () => ({ invoke: async () => [], convertFileSrc: (p: string) => p }));
  lib = await import("$lib/arrastreTirada.svelte");
});

describe("tirada arrastrada sobre un valor", () => {
  it("el botón izquierdo quita el total y el derecho lo suma", () => {
    expect(lib.deltaDeArrastre(0, 12)).toBe(-12);
    expect(lib.deltaDeArrastre(2, 12)).toBe(12);
    expect(lib.deltaDeArrastre(0, -3)).toBe(3); // un total negativo cura con el izquierdo
  });

  it("el destino sale del atributo data-valor, esté el cursor en el elemento o dentro de él", () => {
    const hijo = { closest: () => ({ dataset: { valor: "4:2" } }) } as unknown as Element;
    expect(lib.destinoDeValor(hijo)).toEqual({ idInstancia: 4, idDefinicion: 2 });

    const sin = { closest: () => null } as unknown as Element;
    expect(lib.destinoDeValor(sin)).toBeNull();
    expect(lib.destinoDeValor(null)).toBeNull();
    const roto = { closest: () => ({ dataset: { valor: "x:y" } }) } as unknown as Element;
    expect(lib.destinoDeValor(roto)).toBeNull();
  });
});
