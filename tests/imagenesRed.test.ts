// Imágenes del jugador por la conexión del juego (src/lib/red/imagenesRed.ts): pedirlas, recibirlas en trozos
// (aunque lleguen intercaladas), la cola, los reintentos y las subidas.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  claveImagen,
  ImagenesRed,
  leerTrozo,
  MAX_A_LA_VEZ,
  TIEMPO_SIN_NOTICIAS_MS,
  trozo,
  TROZO,
  TROZO_BAJADA,
  TROZO_SUBIDA
} from "$lib/red/imagenesRed";
import type { MensajeCliente } from "$lib/red/protocolo";

function nueva() {
  const mensajes: MensajeCliente[] = [];
  const binarios: Uint8Array[] = [];
  const red = new ImagenesRed({ mensaje: (m) => mensajes.push(m), binario: (b) => binarios.push(b), sitio: async () => {} }, "/vtt-rol");
  const pedidas = () => mensajes.filter((m): m is Extract<MensajeCliente, { tipo: "PedirImagen" }> => m.tipo === "PedirImagen");
  /** El host manda la imagen `id` entera en trozos de `tam` bytes. */
  const mandar = (id: number, datos: Uint8Array, mime = "image/png") => {
    red.alMensaje({ tipo: "ImagenCabecera", id, mime, tam: datos.length });
    for (let i = 0; i < datos.length; i += TROZO) red.alBinario(trozo(TROZO_BAJADA, id, datos.subarray(i, i + TROZO)).buffer);
  };
  return { red, mensajes, binarios, pedidas, mandar };
}

const bytes = (n: number, semilla = 0) => new Uint8Array(n).map((_, i) => (i * 7 + semilla) % 256);
const contenido = async (b: Blob) => new Uint8Array(await b.arrayBuffer());

describe("imágenes por la conexión", () => {
  beforeEach(() => vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] }));
  afterEach(() => vi.useRealTimers());

  it("un trozo lleva tipo, id en big endian y los datos", () => {
    const t = trozo(TROZO_BAJADA, 0x01020304, new Uint8Array([9, 8]));
    expect([...t]).toEqual([1, 1, 2, 3, 4, 9, 8]);
    const l = leerTrozo(t.buffer)!;
    expect([l.tipo, l.id, [...l.datos]]).toEqual([TROZO_BAJADA, 0x01020304, [9, 8]]);
    expect(leerTrozo(new Uint8Array([1, 2]).buffer)).toBeNull();
    expect(claveImagen("/vtt-rol", "imagenes/a b.png", "ficha")).toBe("/vtt-rol/__imagen?ruta=imagenes%2Fa+b.png&mini=ficha");
  });

  it("pide la imagen y la entrega entera aunque llegue en varios trozos e intercalada con otra", async () => {
    const { red, pedidas, mandar } = nueva();
    const a = red.pedir("imagenes/a.png");
    const b = red.pedir("imagenes/b.png", "ficha");
    await vi.waitFor(() => expect(pedidas().length).toBe(2));
    expect(pedidas()[1]).toMatchObject({ ruta: "imagenes/b.png", mini: "ficha" });
    const [ia, ib] = pedidas().map((p) => p.id);
    const da = bytes(40_000, 1);
    const db = bytes(20_000, 2);
    // Cabeceras y trozos de las dos, mezclados
    red.alMensaje({ tipo: "ImagenCabecera", id: ia, mime: "image/png", tam: da.length });
    red.alMensaje({ tipo: "ImagenCabecera", id: ib, mime: "image/webp", tam: db.length });
    red.alBinario(trozo(TROZO_BAJADA, ia, da.subarray(0, TROZO)).buffer);
    red.alBinario(trozo(TROZO_BAJADA, ib, db.subarray(0, TROZO)).buffer);
    red.alBinario(trozo(TROZO_BAJADA, ia, da.subarray(TROZO, 2 * TROZO)).buffer);
    red.alBinario(trozo(TROZO_BAJADA, ib, db.subarray(TROZO)).buffer);
    red.alBinario(trozo(TROZO_BAJADA, ia, da.subarray(2 * TROZO)).buffer);
    const [ba, bb] = await Promise.all([a, b]);
    expect(await contenido(ba)).toEqual(da);
    expect(ba.type).toBe("image/png");
    expect(await contenido(bb)).toEqual(db);
    expect(bb.type).toBe("image/webp");
  });

  it("la misma imagen pedida dos veces a la vez se pide una sola vez", async () => {
    const { red, pedidas, mandar } = nueva();
    const a = red.pedir("imagenes/a.png");
    const b = red.pedir("imagenes/a.png");
    await vi.waitFor(() => expect(pedidas().length).toBe(1));
    mandar(pedidas()[0].id, bytes(10));
    expect(await Promise.all([a, b])).toHaveLength(2);
    expect(pedidas().length).toBe(1);
  });

  it(`como mucho ${MAX_A_LA_VEZ} a la vez: las demás esperan turno`, async () => {
    const { red, pedidas, mandar } = nueva();
    const todas = Array.from({ length: MAX_A_LA_VEZ + 3 }, (_, i) => red.pedir(`imagenes/${i}.png`));
    await vi.waitFor(() => expect(pedidas().length).toBe(MAX_A_LA_VEZ));
    mandar(pedidas()[0].id, bytes(5));
    // Al terminar una, empieza la siguiente
    await vi.waitFor(() => expect(pedidas().length).toBe(MAX_A_LA_VEZ + 1));
    for (const p of pedidas().slice(1)) mandar(p.id, bytes(5));
    await vi.waitFor(() => expect(pedidas().length).toBe(MAX_A_LA_VEZ + 3));
    for (const p of pedidas().slice(MAX_A_LA_VEZ + 1)) mandar(p.id, bytes(5));
    expect(await Promise.all(todas)).toHaveLength(MAX_A_LA_VEZ + 3);
  });

  it("si el host está ocupado se reintenta al segundo; otro error se entrega tal cual", async () => {
    const { red, pedidas, mandar } = nueva();
    const a = red.pedir("imagenes/a.png");
    await vi.waitFor(() => expect(pedidas().length).toBe(1));
    red.alMensaje({ tipo: "ImagenError", id: pedidas()[0].id, error: "ocupado" });
    vi.advanceTimersByTime(1000);
    await vi.waitFor(() => expect(pedidas().length).toBe(2));
    mandar(pedidas()[1].id, bytes(3));
    expect(await contenido(await a)).toEqual(bytes(3));

    const b = red.pedir("imagenes/oculta.png");
    await vi.waitFor(() => expect(pedidas().length).toBe(3));
    red.alMensaje({ tipo: "ImagenError", id: pedidas()[2].id, error: "No disponible" });
    await expect(b).rejects.toBe("No disponible");
  });

  it("si no llega nada en un rato, se da por perdida y se le dice al host que pare", async () => {
    const { red, mensajes, pedidas } = nueva();
    const a = red.pedir("imagenes/a.png");
    const fallo = expect(a).rejects.toBe("La imagen no ha llegado");
    await vi.waitFor(() => expect(pedidas().length).toBe(1));
    vi.advanceTimersByTime(TIEMPO_SIN_NOTICIAS_MS + 1);
    await fallo;
    expect(mensajes.at(-1)).toEqual({ tipo: "CancelarImagen", id: pedidas()[0].id });
  });

  it("al cortarse la conexión, lo pendiente falla", async () => {
    const { red, pedidas } = nueva();
    const a = red.pedir("imagenes/a.png");
    await vi.waitFor(() => expect(pedidas().length).toBe(1));
    red.cortar("Sin conexión con la sala");
    await expect(a).rejects.toBe("Sin conexión con la sala");
  });

  it("sube en trozos de 16 KiB y devuelve la ruta que da el host", async () => {
    const { red, mensajes, binarios } = nueva();
    const datos = bytes(TROZO * 2 + 100);
    const subida = red.subir(new Blob([datos]), "png");
    await vi.waitFor(() => expect(binarios.length).toBe(3));
    const aviso = mensajes.find((m) => m.tipo === "SubirImagen")!;
    expect(aviso).toMatchObject({ ext: "png", tam: datos.length });
    expect(binarios.every((b) => b[0] === TROZO_SUBIDA)).toBe(true);
    expect(binarios.flatMap((b) => [...b.subarray(5)])).toEqual([...datos]);
    red.alMensaje({ tipo: "SubidaOk", id: (aviso as { id: number }).id, ruta: "imagenes/x.png" });
    await expect(subida).resolves.toBe("imagenes/x.png");
  });
});
