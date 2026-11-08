// Pruebas del visor de la foto a resolución completa (src/lib/visorImagen.svelte.ts).
import { describe, expect, it } from "vitest";
import { visorImagen } from "$lib/visorImagen.svelte";

describe("visor de imagen", () => {
  it("abrir guarda la ruta y cerrar la suelta", () => {
    visorImagen.abrir("imagenes/abc.png");
    expect(visorImagen.ruta).toBe("imagenes/abc.png");
    visorImagen.cerrar();
    expect(visorImagen.ruta).toBeNull();
  });

  it("una ficha sin foto no abre nada y no cierra lo que hubiera abierto", () => {
    visorImagen.abrir("imagenes/abc.png");
    visorImagen.abrir(null);
    visorImagen.abrir("");
    visorImagen.abrir(undefined);
    expect(visorImagen.ruta).toBe("imagenes/abc.png");
    visorImagen.cerrar();
    visorImagen.abrir(null);
    expect(visorImagen.ruta).toBeNull();
  });
});

describe("posición del visor", () => {
  it("recuerda dónde se abrió (la esquina superior derecha va al puntero)", () => {
    visorImagen.abrir("imagenes/abc.png", 900, 120);
    expect([visorImagen.x, visorImagen.y]).toEqual([900, 120]);
    visorImagen.abrir("imagenes/otra.png", 400, 300);
    expect([visorImagen.ruta, visorImagen.x, visorImagen.y]).toEqual(["imagenes/otra.png", 400, 300]);
    visorImagen.cerrar();
  });
});
