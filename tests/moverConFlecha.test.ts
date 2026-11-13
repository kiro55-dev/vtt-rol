// Mover una ficha con la flecha del botón derecho (src/lib/moverConFlecha.ts).
import { describe, expect, it } from "vitest";
import { destinoDeLaFlecha, enterEsDelTablero } from "$lib/moverConFlecha";
import type { Instancia } from "$lib/partida.svelte";

const instancia = (extra: Partial<Instancia> = {}) =>
  ({ id_instancia: 7, id_mapa: 1, coordenadas: { Casilla: { x: 2, y: 2 } }, ...extra }) as unknown as Instancia;
const camino = [
  { x: 2, y: 2 },
  { x: 3, y: 2 },
  { x: 4, y: 2 },
  { x: 4, y: 3 }
];
const todos = () => true;

describe("adónde va la ficha de origen de la flecha", () => {
  it("la casilla pulsada acaba en la última de la flecha y el resto de la ficha queda como estaba (3×3 cogida abajo a la derecha)", () => {
    // Ficha 3×3 con su origen (centro) en (5, 5): ocupa de (4, 4) a (6, 6). Se coge por (6, 6), abajo a la derecha, y la
    // flecha termina en (10, 8): esa esquina cae en (10, 8), así que el centro queda en (9, 7) y el resto, arriba a la izquierda.
    const grande = instancia({ coordenadas: { Casilla: { x: 5, y: 5 } } });
    const flecha = [
      { x: 6, y: 6 },
      { x: 7, y: 6 },
      { x: 8, y: 6 },
      { x: 9, y: 6 },
      { x: 10, y: 6 },
      { x: 10, y: 7 },
      { x: 10, y: 8 }
    ];
    expect(destinoDeLaFlecha({ recorrido: flecha, instancia: grande, idMapa: 1, controla: todos })).toEqual({ id_mapa: 1, casilla: { x: 9, y: 7 } });
    // Cogida por la esquina de arriba a la izquierda (4, 4), hacia (1, 0): el centro se desplaza lo mismo que la flecha
    const hacia = [{ x: 4, y: 4 }, { x: 3, y: 4 }, { x: 2, y: 4 }, { x: 1, y: 4 }, { x: 1, y: 3 }, { x: 1, y: 2 }, { x: 1, y: 1 }, { x: 1, y: 0 }];
    expect(destinoDeLaFlecha({ recorrido: hacia, instancia: grande, idMapa: 1, controla: todos })).toEqual({ id_mapa: 1, casilla: { x: 2, y: 1 } });
  });

  it("una ficha 1×1 (o cogida por su origen) queda justo en la casilla final, sin tocar el recorrido", () => {
    const antes = JSON.stringify(camino);
    expect(destinoDeLaFlecha({ recorrido: camino, instancia: instancia(), idMapa: 1, controla: todos })).toEqual({ id_mapa: 1, casilla: { x: 4, y: 3 } });
    expect(JSON.stringify(camino)).toBe(antes);
  });

  it("sin flecha de al menos dos casillas (solo la de salida, o nada) no hay nada que mover", () => {
    for (const recorrido of [[], [{ x: 2, y: 2 }]]) {
      expect(destinoDeLaFlecha({ recorrido, instancia: instancia(), idMapa: 1, controla: todos })).toBeNull();
    }
  });

  it("la ficha tiene que existir, estar puesta en una casilla de ESTE mapa y haber mapa abierto", () => {
    const base = { recorrido: camino, controla: todos };
    expect(destinoDeLaFlecha({ ...base, instancia: undefined, idMapa: 1 })).toBeNull();
    expect(destinoDeLaFlecha({ ...base, instancia: instancia({ id_mapa: 2 }), idMapa: 1 })).toBeNull(); // en otro mapa
    expect(destinoDeLaFlecha({ ...base, instancia: instancia({ id_mapa: null, coordenadas: "NoPuesto" } as never), idMapa: 1 })).toBeNull();
    expect(destinoDeLaFlecha({ ...base, instancia: instancia(), idMapa: null })).toBeNull();
  });

  it("los mismos permisos que al moverla a mano: si no la controla, no se mueve (y no se le pregunta por otra)", () => {
    const pedidas: number[] = [];
    const controla = (i: Instancia) => {
      pedidas.push(i.id_instancia);
      return i.id_instancia === 7;
    };
    expect(destinoDeLaFlecha({ recorrido: camino, instancia: instancia(), idMapa: 1, controla })).not.toBeNull(); // la suya
    expect(destinoDeLaFlecha({ recorrido: camino, instancia: instancia({ id_instancia: 8 }), idMapa: 1, controla })).toBeNull(); // la ajena
    expect(pedidas).toEqual([7, 8]);
  });

  it("devuelve una casilla nueva (no la del recorrido ni la de la ficha, que se siguen modificando)", () => {
    const i = instancia();
    const d = destinoDeLaFlecha({ recorrido: camino, instancia: i, idMapa: 1, controla: todos })!;
    expect(d.casilla).not.toBe(camino[3]);
    expect(d.casilla).not.toBe((i.coordenadas as { Casilla: unknown }).Casilla);
  });
});

describe("el Enter es del tablero solo si no es de un campo, un botón o un enlace", () => {
  const sobre = (selector: string | null) => ({ closest: (s: string) => (selector !== null && s.includes(selector) ? {} : null) });

  it("un Enter suelto sobre nada en particular, sí", () => {
    expect(enterEsDelTablero({ key: "Enter", target: sobre(null) })).toBe(true);
    expect(enterEsDelTablero({ key: "Enter" })).toBe(true);
  });

  it("otras teclas, modificadores y repeticiones, no", () => {
    expect(enterEsDelTablero({ key: "Escape" })).toBe(false);
    expect(enterEsDelTablero({ key: " " })).toBe(false);
    for (const m of ["ctrlKey", "altKey", "metaKey", "shiftKey"] as const) expect(enterEsDelTablero({ key: "Enter", [m]: true }), m).toBe(false);
    expect(enterEsDelTablero({ key: "Enter", repeat: true })).toBe(false);
  });

  it("dentro de un campo de texto, un botón, un enlace, un selector o algo editable, el Enter es suyo", () => {
    for (const s of ["input", "textarea", "select", "button", "a", "contenteditable"]) {
      expect(enterEsDelTablero({ key: "Enter", target: sobre(s) }), s).toBe(false);
    }
  });
});
