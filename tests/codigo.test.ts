// Los códigos de la conexión directa a mano (src/lib/red/codigo.ts) y los trozos de los mensajes por WebRTC
// (src/lib/red/trozos.ts): mismos formatos que en Rust, con un código fijo hecho por cada lado.
import { describe, expect, it } from "vitest";
import { cifrar, descifrar, type Contenido } from "$lib/red/codigo";

const SECRETO = "0123456789abcdef_-XYZ";
const CONTENIDO: Contenido = { tipo: "oferta", sdp: "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\n", t: 1_700_000_000_000, n: "abc" };
/** Hecho por Rust (`red::codigo::tests::VECTOR_RUST`). */
const VECTOR_RUST =
  "BwcHBwcHBwcHBwcHiYMFR3cAT11gR3LXVL96XEJIQm9JJTRaeHhSaC8i1mmlCqMZz3bulsZDfKBd6lODQ_23Ch58MBYBQ7pINztM4eodFu2-ZG9Lx0XnECOMEP6XY1oXjzWV_KpIaYZBi104leZGxw";

describe("códigos de la conexión directa", () => {
  it("descifra el código fijo que hace Rust", async () => {
    expect(await descifrar(SECRETO, VECTOR_RUST)).toEqual(CONTENIDO);
  });

  it("ida y vuelta, y solo con el secreto de la sala", async () => {
    const c = await cifrar(SECRETO, CONTENIDO);
    expect(await descifrar(SECRETO, `  ${c}\n`)).toEqual(CONTENIDO);
    await expect(descifrar("otro-secreto-de-otra-sala", c)).rejects.toBe("Ese código no es de esta sala (o está incompleto)");
    const roto = c.slice(0, 20) + (c[20] === "A" ? "B" : "A") + c.slice(21);
    await expect(descifrar(SECRETO, roto)).rejects.toBeTruthy();
    await expect(descifrar(SECRETO, "hola")).rejects.toBe("Eso no es un código válido");
  });

  it("el código fijo para Rust", async () => {
    const c = await cifrar(SECRETO, CONTENIDO, new Uint8Array(12).fill(9));
    expect(c).toBe(VECTOR_TS);
  });
});

/** Lo descifra `red::codigo::tests::descifra_el_codigo_del_navegador`. */
const VECTOR_TS = "CQkJCQkJCQkJCQkJu_lSHVLR2Q2YGKJzDEecsZvFdLnZGX6KJUduvMIuO-1LdzBdUUmESrN629q2L9SdYQEX9IJDOEiAInedj5psSC8nto5KPjxup0EijgtyILzHIzFkbso83_bRiP0EmJPy";

import { Recomponer, SIGUE, TROZO, trocear, ULTIMO } from "$lib/red/trozos";

describe("trozos de los mensajes por WebRTC", () => {
  it("un mensaje largo va en trozos de 16 KiB y se recompone (con caracteres partidos entre trozos)", () => {
    const texto = "a".repeat(50_000) + "ñ€😀";
    const trozos = trocear(texto);
    expect(trozos.length).toBe(4);
    expect(trozos.every((t) => t.length <= TROZO)).toBe(true);
    expect(trozos.filter((t) => t[0] === ULTIMO).length).toBe(1);
    const r = new Recomponer(1024 * 1024);
    const salidas = trozos.map((t) => r.trozo(t));
    expect(salidas.at(-1)).toBe(texto);
    expect(salidas.slice(0, -1).every((s) => s === null)).toBe(true);
    expect(r.trozo(trocear("")[0])).toBe("");
  });

  it("un trozo malo o un mensaje demasiado grande es un error", () => {
    expect(() => new Recomponer(10).trozo(new Uint8Array([]))).toThrow();
    expect(() => new Recomponer(10).trozo(new Uint8Array([7, 1]))).toThrow();
    expect(() => new Recomponer(10).trozo(new Uint8Array([SIGUE, ...new Array(12).fill(0)]))).toThrow();
    expect(() => new Recomponer(10).trozo(new Uint8Array([ULTIMO, 0xff, 0xfe]))).toThrow();
  });
});
