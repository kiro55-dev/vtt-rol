// Los avisos pasajeros del tablero (src/lib/notificaciones.svelte.ts): se apilan por orden de llegada y cada uno tiene su
// propio reloj de 3 s.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DURACION_NOTIFICACION_MS, MAX_NOTIFICACIONES, notificaciones, textoUnido } from "$lib/notificaciones.svelte";

const textos = () => notificaciones.lista.map((n) => n.texto);

describe("notificaciones", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    notificaciones.limpiar();
  });
  afterEach(() => {
    notificaciones.limpiar();
    vi.useRealTimers();
  });

  it("el texto de un jugador que se une", () => {
    expect(textoUnido("Ana")).toBe("Ana se unió a la sala");
  });

  it("un aviso se ve y se quita a los 3 s justos", () => {
    notificaciones.mostrar(textoUnido("Ana"));
    expect(textos()).toEqual(["Ana se unió a la sala"]);
    vi.advanceTimersByTime(DURACION_NOTIFICACION_MS - 1);
    expect(textos()).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(textos()).toEqual([]);
  });

  it("varios a la vez se apilan por orden de llegada (el más nuevo debajo) y se quitan a la vez", () => {
    for (const n of ["Ana", "Luis", "Marta"]) notificaciones.mostrar(textoUnido(n));
    expect(textos()).toEqual(["Ana se unió a la sala", "Luis se unió a la sala", "Marta se unió a la sala"]);
    vi.advanceTimersByTime(DURACION_NOTIFICACION_MS);
    expect(textos()).toEqual([]);
  });

  it("cada uno cuenta sus 3 s por separado: uno nuevo no alarga ni acorta a los anteriores", () => {
    notificaciones.mostrar("A");
    vi.advanceTimersByTime(1000);
    notificaciones.mostrar("B");
    vi.advanceTimersByTime(1000);
    notificaciones.mostrar("C");
    expect(textos()).toEqual(["A", "B", "C"]); // t = 2000
    vi.advanceTimersByTime(1000); // t = 3000: se va A
    expect(textos()).toEqual(["B", "C"]);
    vi.advanceTimersByTime(1000); // t = 4000: se va B
    expect(textos()).toEqual(["C"]);
    vi.advanceTimersByTime(1000); // t = 5000: se va C
    expect(textos()).toEqual([]);
  });

  it("pasarse del tope quita el más antiguo, y su reloj ya no dispara nada", () => {
    for (let i = 1; i <= MAX_NOTIFICACIONES + 2; i++) notificaciones.mostrar(`n${i}`);
    expect(notificaciones.lista).toHaveLength(MAX_NOTIFICACIONES);
    expect(textos()[0]).toBe("n3");
    expect(textos().at(-1)).toBe(`n${MAX_NOTIFICACIONES + 2}`);
    expect(vi.getTimerCount()).toBe(MAX_NOTIFICACIONES); // los relojes de los quitados se cancelaron
  });

  it("quitar a mano cancela su reloj y no toca a los demás", () => {
    const a = notificaciones.mostrar("A");
    notificaciones.mostrar("B");
    notificaciones.quitar(a);
    expect(textos()).toEqual(["B"]);
    expect(vi.getTimerCount()).toBe(1);
    notificaciones.quitar(a); // otra vez: no pasa nada
    expect(textos()).toEqual(["B"]);
  });

  it("un aviso largo (una frase entera) y uno de atención se marcan como tales; los normales, no", () => {
    notificaciones.mostrar("Ana se unió a la sala");
    notificaciones.mostrar("Actualiza OpenRoll: los jugadores no podrán entrar", 6000, { largo: true, aviso: true });
    expect(notificaciones.lista.map((n) => [n.largo, n.aviso])).toEqual([
      [false, false],
      [true, true]
    ]);
    vi.advanceTimersByTime(3000);
    expect(textos()).toEqual(["Actualiza OpenRoll: los jugadores no podrán entrar"]); // y dura lo que se pidió
    vi.advanceTimersByTime(12_000);
    expect(textos()).toEqual([]);
  });

  it("con duración null no se quita sola, lleva «x» y se cierra a mano", () => {
    const id = notificaciones.mostrar("Hay una versión nueva", null, { largo: true });
    notificaciones.mostrar("normal");
    expect(notificaciones.lista.map((n) => n.cerrable)).toEqual([true, false]);
    vi.advanceTimersByTime(3_600_000);
    expect(textos()).toEqual(["Hay una versión nueva"]); // la normal se fue sola; esta no
    notificaciones.quitar(id);
    expect(textos()).toEqual([]);
  });

  it("se puede pedir otra duración", () => {
    notificaciones.mostrar("largo", 5000);
    vi.advanceTimersByTime(4999);
    expect(textos()).toEqual(["largo"]);
    vi.advanceTimersByTime(1);
    expect(textos()).toEqual([]);
  });
});
