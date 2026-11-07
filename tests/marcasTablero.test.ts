// Pruebas de la animación del ping (src/lib/marcasTablero.ts): solo las cuentas, sin canvas.
import { describe, expect, it } from "vitest";
import { DURACION_PING_MS, onda, pingVivo } from "$lib/marcasTablero";

describe("ping", () => {
  it("dura un segundo", () => {
    const ping = { casilla: { x: 0, y: 0 }, inicio: 1000 };
    expect(DURACION_PING_MS).toBe(1000);
    expect(pingVivo(ping, 1999)).toBe(true);
    expect(pingVivo(ping, 2000)).toBe(false);
  });

  it("cada onda crece de 0 al radio máximo mientras se desvanece", () => {
    expect(onda(0, 0)).toEqual({ radio: 0, alfa: 1 });
    const mitad = onda(0.375, 0)!;
    expect(mitad.radio).toBeGreaterThan(0.5);
    expect(mitad.alfa).toBeCloseTo(0.5);
    expect(onda(0.75, 0)).toEqual({ radio: 1, alfa: 0 });
    expect(onda(0.8, 0)).toBeNull();
    // La segunda onda sale con retraso y acaba justo al final del ping
    expect(onda(0.1, 0.25)).toBeNull();
    expect(onda(1, 0.25)).toEqual({ radio: 1, alfa: 0 });
  });
});
