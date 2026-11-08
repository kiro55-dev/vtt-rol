// Cuenta del recorte de la miniatura (src/lib/recorteFoto.ts): lo que ve el editor y lo que se guarda.
import { describe, expect, it } from "vitest";
import { cuadradoVisible, tamanoEnCaja, encuadreCentrado, ZOOM_MAX } from "$lib/recorteFoto";

const apaisada = { ancho: 400, alto: 200 };

describe("cuadradoVisible", () => {
  it("con zoom 100 se ve el lado corto entero y el encuadre elige dónde", () => {
    expect(cuadradoVisible(apaisada, { x: 0, y: 50, zoom: 100 })).toEqual({ sx: 0, sy: 0, lado: 200 });
    expect(cuadradoVisible(apaisada, { x: 50, y: 50, zoom: 100 })).toEqual({ sx: 100, sy: 0, lado: 200 });
    expect(cuadradoVisible(apaisada, { x: 100, y: 50, zoom: 100 })).toEqual({ sx: 200, sy: 0, lado: 200 });
  });

  it("con más zoom se ve menos de la foto y el centro sigue siendo el centro", () => {
    const c = cuadradoVisible(apaisada, { x: 50, y: 50, zoom: 200 });
    expect(c).toEqual({ sx: 150, sy: 50, lado: 100 });
    const max = cuadradoVisible(apaisada, { x: 0, y: 0, zoom: ZOOM_MAX });
    expect(max).toEqual({ sx: 0, sy: 0, lado: 50 });
  });

  it("acota el zoom y el encuadre a sus rangos", () => {
    expect(cuadradoVisible(apaisada, { x: 500, y: -5, zoom: 9000 })).toEqual({ sx: 350, sy: 0, lado: 50 });
    expect(cuadradoVisible(apaisada, { x: 50, y: 50, zoom: 1 }).lado).toBe(200);
  });

  it("la foto por defecto sale centrada", () => {
    expect(encuadreCentrado()).toEqual({ x: 50, y: 50, zoom: 100 });
  });
});

describe("tamanoEnCaja", () => {
  it("cubre la caja con el zoom 100 y escala con el zoom", () => {
    const t100 = tamanoEnCaja(apaisada, 110, 100);
    expect(t100.ancho).toBeCloseTo(220);
    expect(t100.alto).toBeCloseTo(110);
    const t200 = tamanoEnCaja(apaisada, 110, 200);
    expect(t200.ancho).toBeCloseTo(440);
    expect(t200.alto).toBeCloseTo(220);
  });
});
