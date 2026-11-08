// Pruebas de qué pasa al soltar una ficha (src/lib/arrastreFicha.svelte.ts) y de las fichas del
// tablero (src/lib/fichasTablero.ts).
import { beforeEach, describe, expect, it, vi } from "vitest";

let accionAlSoltar: typeof import("$lib/arrastreFicha.svelte").accionAlSoltar;
let moverEnLista: typeof import("$lib/arrastreFicha.svelte").moverEnLista;
let fichas: typeof import("$lib/fichasTablero");

beforeEach(async () => {
  vi.resetModules();
  vi.doMock("@tauri-apps/api/core", () => ({ invoke: async () => [], convertFileSrc: (p: string) => p }));
  ({ accionAlSoltar, moverEnLista } = await import("$lib/arrastreFicha.svelte"));
  fichas = await import("$lib/fichasTablero");
});

const base = (id: number, cuerpo: any = "Pequenio"): any => ({
  id, nombre: "F" + id, tipo: "Enemigo", parametros: [], cuerpo, zona_id: 1, foto_url: null, archivos_adjuntos: [], es_unico: false
});
const instancia = (id: number, idMapa: number | null, casilla: { x: number; y: number } | null, cuerpo?: any): any => ({
  id_instancia: id, id_mapa: idMapa, base: base(1, cuerpo), valores_actuales: [],
  coordenadas: casilla ? { Casilla: casilla } : "NoPuesto", activo: true
});
const destino = { id_mapa: 1, casilla: { x: 2, y: 3 } };

describe("accionAlSoltar", () => {
  it("una ficha del banco se instancia en el tablero o, en «Fichas activas», sin casilla", () => {
    const origen = { entidad: base(7) };
    expect(accionAlSoltar(origen, "tablero", destino)).toEqual({ tipo: "instanciar", idEntidad: 7, destino });
    expect(accionAlSoltar(origen, "activas", null)).toEqual({ tipo: "instanciar", idEntidad: 7, destino: null });
    // Fuera del mapa, en el propio banco o en ninguna parte: nada
    expect(accionAlSoltar(origen, "tablero", null)).toBeNull();
    expect(accionAlSoltar(origen, "banco", null)).toBeNull();
    expect(accionAlSoltar(origen, null, null)).toBeNull();
  });

  it("varias fichas del banco (selección con Ctrl) se instancian todas de golpe", () => {
    const origen = { entidades: [base(7), base(3), base(9)] };
    expect(accionAlSoltar(origen, "tablero", destino)).toEqual({ tipo: "instanciarVarias", idsEntidades: [7, 3, 9], destino });
    expect(accionAlSoltar(origen, "activas", null)).toEqual({ tipo: "instanciarVarias", idsEntidades: [7, 3, 9], destino: null });
    expect(accionAlSoltar(origen, "tablero", null)).toBeNull();
    expect(accionAlSoltar(origen, "banco", null)).toBeNull();
  });

  it("una instancia se mueve en el tablero, sale de él en «Fichas activas» y se elimina en el banco", () => {
    const colocada = { instancia: instancia(4, 1, { x: 0, y: 0 }), desde: "tablero" as const };
    expect(accionAlSoltar(colocada, "tablero", destino)).toEqual({ tipo: "mover", idInstancia: 4, destino });
    expect(accionAlSoltar(colocada, "activas", null)).toEqual({ tipo: "quitarDelTablero", idInstancia: 4 });
    expect(accionAlSoltar(colocada, "banco", null)).toEqual({ tipo: "borrar", idInstancia: 4 });
    expect(accionAlSoltar(colocada, "tablero", null)).toBeNull();

    // Una que ya está fuera del tablero, soltada en «Fichas activas»: no hay nada que hacer
    const sinColocar = { instancia: instancia(5, null, null), desde: "tablero" as const };
    expect(accionAlSoltar(sinColocar, "activas", null)).toBeNull();
    expect(accionAlSoltar(sinColocar, "banco", null)).toEqual({ tipo: "borrar", idInstancia: 5 });
  });
});

describe("reordenar «Fichas activas»", () => {
  it("mover un id a otra posición", () => {
    expect(moverEnLista([1, 2, 3, 4], 1, 2)).toEqual([2, 3, 1, 4]);
    expect(moverEnLista([1, 2, 3, 4], 4, 0)).toEqual([4, 1, 2, 3]);
    expect(moverEnLista([1, 2, 3], 2, 99)).toEqual([1, 3, 2]);
  });

  it("una tarjeta arrastrada dentro de la lista la reordena; si no cambia de sitio, nada", () => {
    const deActivas = { instancia: instancia(2, 1, { x: 0, y: 0 }), desde: "activas" as const };
    expect(accionAlSoltar(deActivas, "activas", null, [1, 2, 3], 0)).toEqual({ tipo: "reordenar", ids: [2, 1, 3] });
    expect(accionAlSoltar(deActivas, "activas", null, [1, 2, 3], 1)).toBeNull();
    expect(accionAlSoltar(deActivas, "activas", null, [1, 2, 3], null)).toBeNull();
    // Desde la lista al tablero o al banco, lo de siempre
    expect(accionAlSoltar(deActivas, "tablero", destino)).toEqual({ tipo: "mover", idInstancia: 2, destino });
    expect(accionAlSoltar(deActivas, "banco", null)).toEqual({ tipo: "borrar", idInstancia: 2 });
  });
});

describe("fichas del tablero", () => {
  it("solo las colocadas en ese mapa, en orden", () => {
    const lista = [instancia(1, 1, { x: 0, y: 0 }), instancia(2, 2, { x: 0, y: 0 }), instancia(3, null, null), instancia(4, 1, { x: 5, y: 5 })];
    expect(fichas.fichasDelMapa(lista, 1).map((f) => f.instancia.id_instancia)).toEqual([1, 4]);
    expect(fichas.fichasDelMapa(lista, null)).toEqual([]);
  });

  it("una ficha girada ocupa sus casillas giradas", () => {
    const tumbada = { ...instancia(1, 1, { x: 5, y: 5 }, { Grande: [1, 3] }), rotacion: 1 }; // 3×1: x 4..6
    const enMapa = fichas.fichasDelMapa([tumbada], 1);
    expect(fichas.fichaEnCasilla(enMapa, { x: 4, y: 5 })?.id_instancia).toBe(1);
    expect(fichas.fichaEnCasilla(enMapa, { x: 5, y: 4 })).toBeNull();
  });

  it("la ficha bajo una casilla cubre todo su cuerpo, y gana la de encima", () => {
    const grande = instancia(1, 1, { x: 5, y: 5 }, { Grande: [3, 3] }); // ocupa 4..6
    const pequena = instancia(2, 1, { x: 6, y: 6 });
    const enMapa = fichas.fichasDelMapa([grande, pequena], 1);
    expect(fichas.fichaEnCasilla(enMapa, { x: 4, y: 4 })?.id_instancia).toBe(1);
    expect(fichas.fichaEnCasilla(enMapa, { x: 6, y: 6 })?.id_instancia).toBe(2); // la última, encima
    expect(fichas.fichaEnCasilla(enMapa, { x: 7, y: 5 })).toBeNull();
  });

  it("cada tipo tiene su color", () => {
    expect(fichas.COLOR_TIPO).toEqual({ Enemigo: "#ef4444", NPC: "#3b82f6", Jugador: "#22c55e" });
  });
});

describe("coger una ficha que quedó fuera del mapa", () => {
  it("se busca por el rectángulo de la ficha, no por su casilla, y gana la de encima", () => {
    const r = { columnas: 10, filas: 8, anchoCasilla: 10, altoCasilla: 10, ancho: 100, alto: 80 };
    // Una ficha en la casilla (14, 3): fuera del mapa (que acaba en x=100), pero a la vista
    const fuera = fichas.fichasDelMapa([instancia(1, 1, { x: 14, y: 3 })], 1);
    expect(fichas.fichaEnPunto(fuera, r, 145, 35)?.id_instancia).toBe(1);
    expect(fichas.fichaEnPunto(fuera, r, 155, 35)).toBeNull();

    const dos = fichas.fichasDelMapa([instancia(1, 1, { x: 2, y: 2 }), instancia(2, 1, { x: 2, y: 2 })], 1);
    expect(fichas.fichaEnPunto(dos, r, 25, 25)?.id_instancia).toBe(2);
  });
});
