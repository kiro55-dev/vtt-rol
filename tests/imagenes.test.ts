// Pruebas de los formatos de imagen por uso (src/lib/imagenes.ts).
import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn(async (_cmd: string, _args?: any) => "imagenes/x.png");
let lib: typeof import("$lib/imagenes");

beforeEach(async () => {
  vi.resetModules();
  invoke.mockClear();
  vi.doMock("@tauri-apps/api/core", () => ({ invoke, convertFileSrc: (r: string) => r }));
  lib = await import("$lib/imagenes");
});

describe("formatos de imagen", () => {
  it("un mapa no admite SVG ni AVIF, la foto de una ficha sí", () => {
    for (const ext of ["png", "JPG", "jpeg", "webp", "gif", "bmp"]) {
      expect(lib.esImagen(`C:\mapas\a.${ext}`, "mapa")).toBe(true);
    }
    expect(lib.esImagen("/m/dibujo.svg", "mapa")).toBe(false);
    expect(lib.esImagen("/m/foto.AVIF", "mapa")).toBe(false);

    expect(lib.esImagen("/m/dibujo.svg", "ficha")).toBe(true);
    expect(lib.esImagen("/m/foto.AVIF", "ficha")).toBe(true);
  });

  it("lo que no es una imagen no vale para nada", () => {
    for (const ruta of ["/m/doc.pdf", "/m/sin_extension", "/m/.png.txt", ""]) {
      expect(lib.esImagen(ruta, "mapa")).toBe(false);
      expect(lib.esImagen(ruta, "ficha")).toBe(false);
    }
  });

  it("importar un formato no admitido se rechaza en el front, sin llamar a Rust", async () => {
    await expect(lib.importarImagen("/m/dibujo.svg", "mapa")).rejects.toContain("un mapa");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("importar pasa a Rust para qué es la imagen", async () => {
    await lib.importarImagen("/m/dibujo.svg", "ficha");
    expect(invoke).toHaveBeenCalledWith("importar_imagen", { origen: "/m/dibujo.svg", uso: "ficha" });
    await lib.importarImagen("/m/mapa.png", "mapa");
    expect(invoke).toHaveBeenLastCalledWith("importar_imagen", { origen: "/m/mapa.png", uso: "mapa" });
  });
});
