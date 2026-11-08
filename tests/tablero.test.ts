// Pruebas de la lógica del tablero (src/lib/tablero.ts). Los casos de casillas son los mismos que
// los de src-tauri/src/tablero_general/tablero.rs: si divergen, front y Rust no coinciden.
import { describe, expect, it } from "vitest";
import {
  cabe,
  casillaEn,
  casillaMasCercana,
  centroDe,
  extenderRecorrido,
  pasosEntre,
  casillasOcupadas,
  encajar,
  extension,
  mundoAPantalla,
  ocupa,
  origenAlSoltar,
  pantallaAMundo,
  rectanguloDe,
  rejillaDe,
  tamanoCuerpo
} from "$lib/tablero";

const c = (x: number, y: number) => ({ x, y });
const cuadricula = (columnas: number, filas: number) => rejillaDe(columnas * 10, filas * 10, 10);

describe("rejilla", () => {
  it("se redondea para que las casillas encajen justas (igual que Rust)", () => {
    const dims = (w: number, h: number, t: number) => {
      const r = rejillaDe(w, h, t);
      return [r.columnas, r.filas];
    };
    expect(dims(1000, 1000, 50)).toEqual([20, 20]);
    expect(dims(1020, 1030, 50)).toEqual([20, 21]);
    expect(dims(1025, 75, 50)).toEqual([21, 2]); // la mitad exacta hacia arriba
    expect(dims(10, 10, 300)).toEqual([1, 1]);
    expect(dims(0, 0, 50)).toEqual([1, 1]);
    expect(dims(7, 3, 0)).toEqual([7, 3]);
  });

  it("la casilla mide lo justo para cubrir el mapa", () => {
    const r = rejillaDe(1030, 1000, 50); // 21 columnas
    expect(r.anchoCasilla * r.columnas).toBeCloseTo(1030);
    expect(r.anchoCasilla).toBeCloseTo(1030 / 21);
    expect(r.altoCasilla).toBe(50);
  });
});

describe("cuerpo", () => {
  it("tamaño y casilla central", () => {
    expect(tamanoCuerpo("Pequenio")).toEqual([1, 1]);
    expect(tamanoCuerpo({ Grande: [2, 3] })).toEqual([2, 3]);
    expect(tamanoCuerpo({ Grande: [0] })).toEqual([1, 1]);
    expect([1, 2, 3, 4, 0].map(extension)).toEqual([[0, 0], [0, 1], [1, 1], [1, 2], [0, 0]]);
  });

  it("casillas ocupadas alrededor del centro", () => {
    const tres = casillasOcupadas(c(5, 5), [3, 3]);
    expect(tres).toHaveLength(9);
    expect([tres[0], tres[8]]).toEqual([c(4, 4), c(6, 6)]);
    expect(casillasOcupadas(c(5, 5), [2, 3])).toEqual([c(5, 4), c(6, 4), c(5, 5), c(6, 5), c(5, 6), c(6, 6)]);
    expect(casillasOcupadas(c(0, 0), [3, 3])).toHaveLength(4);

    expect(ocupa(c(5, 5), [2, 3], c(6, 6))).toBe(true);
    expect(ocupa(c(5, 5), [2, 3], c(4, 5))).toBe(false);
  });

  it("cabe y encajar trabajan con el rectángulo entero", () => {
    const r = cuadricula(10, 8);
    expect(cabe(r, c(9, 7), [1, 1])).toBe(true);
    expect(cabe(r, c(10, 0), [1, 1])).toBe(false);
    expect(cabe(r, c(0, 4), [3, 3])).toBe(false);
    expect(cabe(r, c(8, 6), [3, 3])).toBe(true);
    expect(cabe(r, c(0, 1), [2, 3])).toBe(true);
    expect(cabe(r, c(9, 1), [2, 3])).toBe(false);

    expect(encajar(r, c(4, 4), [3, 3])).toEqual(c(4, 4));
    expect(encajar(r, c(0, 0), [3, 3])).toEqual(c(1, 1));
    expect(encajar(r, c(9, 7), [3, 3])).toEqual(c(8, 6));
    expect(encajar(r, c(50, 50), [2, 3])).toEqual(c(8, 6));
    expect(encajar(r, c(-3, 0), [2, 3])).toEqual(c(0, 1));
    expect(encajar(r, c(9, 5), [12, 3])).toEqual(c(5, 5)); // más grande que el mapa
  });
});

describe("píxeles", () => {
  const cam = { x: 120, y: -40, zoom: 2.5 };

  it("pantalla y mundo son inversos", () => {
    const m = pantallaAMundo(cam, 300, 200);
    expect(m).toEqual({ x: 72, y: 96 });
    expect(mundoAPantalla(cam, m.x, m.y)).toEqual({ x: 300, y: 200 });
  });

  it("casilla bajo un punto del mapa", () => {
    const r = cuadricula(10, 8); // casillas de 10 px
    expect(casillaEn(r, 0, 0)).toEqual(c(0, 0));
    expect(casillaEn(r, 9.99, 10)).toEqual(c(0, 1));
    expect(casillaEn(r, 99.9, 79.9)).toEqual(c(9, 7));
    expect(casillaEn(r, 100, 5)).toBeNull();
    expect(casillaEn(r, -0.1, 5)).toBeNull();
  });

  it("el rectángulo de dibujo cubre todo el cuerpo", () => {
    const r = cuadricula(10, 8);
    expect(rectanguloDe(r, c(3, 2), [1, 1])).toEqual({ x: 30, y: 20, w: 10, h: 10 });
    expect(rectanguloDe(r, c(5, 5), [2, 3])).toEqual({ x: 50, y: 40, w: 20, h: 30 });
  });

  it("al soltar, el centro del cuerpo sigue al cursor y encaja en el mapa", () => {
    const r = cuadricula(10, 8);
    // 1×1 y 3×3: la casilla bajo el cursor
    expect(origenAlSoltar(r, 35, 25, [1, 1])).toEqual(c(3, 2));
    expect(origenAlSoltar(r, 59.9, 50, [3, 3])).toEqual(c(5, 5));
    // 2×2: la que deja el cursor más cerca del centro del cuadrado
    expect(origenAlSoltar(r, 51, 51, [2, 2])).toEqual(c(4, 4)); // ocupa 4..5: centro en 50
    expect(origenAlSoltar(r, 58, 58, [2, 2])).toEqual(c(5, 5)); // ocupa 5..6: centro en 60
    // 2×3 junto al borde: no se sale
    expect(origenAlSoltar(r, 99, 79, [2, 3])).toEqual(c(8, 6));
    expect(origenAlSoltar(r, 1, 1, [2, 3])).toEqual(c(0, 1));
    expect(cabe(r, origenAlSoltar(r, 99, 1, [2, 3]), [2, 3])).toBe(true);
  });
});

describe("recorrido", () => {
  it("casilla más cercana a un punto, aunque esté fuera del mapa", () => {
    const r = cuadricula(10, 8);
    expect(casillaMasCercana(r, 35, 25)).toEqual(c(3, 2));
    expect(casillaMasCercana(r, -50, 500)).toEqual(c(0, 7));
    expect(casillaMasCercana(r, 1000, -1)).toEqual(c(9, 0));
    expect(centroDe(r, c(3, 2))).toEqual({ x: 35, y: 25 });
  });

  it("los pasos entre dos casillas van solo en horizontal o vertical y sin huecos", () => {
    expect(pasosEntre(c(2, 2), c(2, 2))).toEqual([]);
    expect(pasosEntre(c(2, 2), c(5, 2))).toEqual([c(3, 2), c(4, 2), c(5, 2)]);
    expect(pasosEntre(c(2, 2), c(2, 0))).toEqual([c(2, 1), c(2, 0)]);

    const diagonal = pasosEntre(c(0, 0), c(3, 2));
    expect(diagonal).toHaveLength(5); // 3 + 2
    expect(diagonal.at(-1)).toEqual(c(3, 2));
    let previa = c(0, 0);
    for (const p of diagonal) {
      expect(Math.abs(p.x - previa.x) + Math.abs(p.y - previa.y)).toBe(1);
      previa = p;
    }
  });

  it("el recorrido suma las casillas recorridas, como en el dibujo de ejemplo", () => {
    // Sube 1, derecha 2, baja 3 y una a la izquierda: 7 casillas
    let r = [c(1, 2)];
    for (const destino of [c(1, 1), c(3, 1), c(3, 4), c(2, 4)]) r = extenderRecorrido(r, destino);
    expect(r.length - 1).toBe(7);
    expect(r[0]).toEqual(c(1, 2));
    expect(r.at(-1)).toEqual(c(2, 4));
  });

  it("volver por el mismo camino acorta el recorrido", () => {
    let r = extenderRecorrido([c(0, 0)], c(4, 0));
    expect(r.length - 1).toBe(4);
    r = extenderRecorrido(r, c(2, 0));
    expect(r).toEqual([c(0, 0), c(1, 0), c(2, 0)]);
    r = extenderRecorrido(r, c(0, 0));
    expect(r).toEqual([c(0, 0)]);
    // Quedarse en la misma casilla no cambia nada
    expect(extenderRecorrido(r, c(0, 0))).toEqual([c(0, 0)]);
    expect(extenderRecorrido([], c(3, 3))).toEqual([c(3, 3)]);
  });
});

describe("instancias giradas y formas", () => {
  it("los lados de una instancia se intercambian con un giro impar", async () => {
    const { ladosInstancia } = await import("$lib/tablero");
    const ficha = (rotacion: number) => ({ base: { cuerpo: { Grande: [2, 3] } }, rotacion });
    expect([0, 1, 2, 3].map((r) => ladosInstancia(ficha(r)))).toEqual([[2, 3], [3, 2], [2, 3], [3, 2]]);
    expect(ladosInstancia({ base: { cuerpo: "Pequenio" } })).toEqual([1, 1]);
  });

  it("los cuatro puntos de una forma caen en la rejilla, a n casillas de los bordes del cuerpo", async () => {
    const { extremosForma } = await import("$lib/tablero");
    // 1×1 en (5,5), valor 2: de la línea 3 a la 8 en cada eje; centro 5.5
    expect(extremosForma(c(5, 5), [1, 1], 2)).toEqual({ izquierda: 3, derecha: 8, arriba: 3, abajo: 8, cx: 5.5, cy: 5.5 });
    // 2×3 en (5,5) (x 5..6, y 4..6), valor 1
    expect(extremosForma(c(5, 5), [2, 3], 1)).toEqual({ izquierda: 4, derecha: 8, arriba: 3, abajo: 8, cx: 6, cy: 5.5 });
  });
});
