// Las fichas que otros arrastran: se ven por donde pasan, y se quitan al soltar, al llegar su posición o al caducar
// (src/lib/arrastreAjeno.svelte.ts).
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AvisoTablero } from "$lib/avisosTablero";

const arrastre = (piezas: [number, number, number][], id_mapa = 1): AvisoTablero => ({
  Arrastre: { id_mapa, piezas: piezas.map(([id_instancia, x, y]) => ({ id_instancia, casilla: { x, y } })) }
});
const fin = (ids: number[], id_mapa = 1): AvisoTablero => ({ ArrastreFin: { id_mapa, ids_instancias: ids } });

async function nuevo() {
  vi.resetModules();
  const m = await import("$lib/arrastreAjeno.svelte");
  return { store: m.arrastreAjeno, ...m };
}

describe("arrastre de otros", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("guarda por dónde pasa cada ficha, solo en su mapa, y sigue al último aviso", async () => {
    const { store } = await nuevo();
    store.recibir(arrastre([[1, 2, 3], [2, 4, 4]]));
    expect(store.de(1, 1)).toEqual({ x: 2, y: 3 });
    expect(store.de(2, 1)).toEqual({ x: 4, y: 4 });
    expect(store.de(1, 9)).toBeNull(); // otro mapa
    expect(store.de(7, 1)).toBeNull(); // nadie la arrastra
    store.recibir(arrastre([[1, 5, 5]]));
    expect(store.de(1, 1)).toEqual({ x: 5, y: 5 });
    expect(store.de(2, 1)).toEqual({ x: 4, y: 4 }); // las que no se mencionan siguen igual
  });

  it("ignora los avisos que no son de arrastre", async () => {
    const { store } = await nuevo();
    store.recibir({ Ping: { id_mapa: 1, casilla: { x: 1, y: 1 } } });
    store.recibir({ Foco: { ids_instancias: [1] } });
    expect(store.piezas).toEqual({});
  });

  it("al soltar se mantiene un momento y se quita cuando llega su posición real", async () => {
    const { store } = await nuevo();
    store.recibir(arrastre([[1, 2, 3]]));
    store.recibir(fin([1]));
    expect(store.de(1, 1)).toEqual({ x: 2, y: 3 }); // aún no «salta atrás»
    // La posición real todavía es la vieja: sigue
    store.alLlegarPosiciones(new Map([[1, { x: 0, y: 0 }]]));
    expect(store.de(1, 1)).not.toBeNull();
    // Llega la nueva: sobra
    store.alLlegarPosiciones(new Map([[1, { x: 2, y: 3 }]]));
    expect(store.de(1, 1)).toBeNull();
  });

  it("una ficha que aún se arrastra no se quita porque pase por su casilla de origen", async () => {
    const { store } = await nuevo();
    store.recibir(arrastre([[1, 0, 0]]));
    store.alLlegarPosiciones(new Map([[1, { x: 0, y: 0 }]]));
    expect(store.de(1, 1)).toEqual({ x: 0, y: 0 });
  });

  it("tras soltar caduca sola, y sin latido también (emisor desconectado)", async () => {
    const { store, PERMANENCIA_TRAS_SOLTAR_MS, CADUCA_SIN_LATIDO_MS } = await nuevo();
    store.recibir(arrastre([[1, 2, 3]]));
    store.recibir(fin([1]));
    vi.advanceTimersByTime(PERMANENCIA_TRAS_SOLTAR_MS + 50);
    expect(store.de(1, 1)).toBeNull();

    store.recibir(arrastre([[2, 1, 1]]));
    vi.advanceTimersByTime(CADUCA_SIN_LATIDO_MS - 100);
    expect(store.de(2, 1)).not.toBeNull();
    store.recibir(arrastre([[2, 1, 1]])); // el latido renueva
    vi.advanceTimersByTime(CADUCA_SIN_LATIDO_MS - 100);
    expect(store.de(2, 1)).not.toBeNull();
    vi.advanceTimersByTime(200);
    expect(store.de(2, 1)).toBeNull();
  });

  it("limpiar lo vacía (otro mapa u otra partida)", async () => {
    const { store } = await nuevo();
    store.recibir(arrastre([[1, 2, 3]]));
    store.limpiar();
    expect(store.de(1, 1)).toBeNull();
  });
});
