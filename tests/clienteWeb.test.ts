// Pruebas del cliente web del jugador (cliente-web/src): el enlace, aplicar los cambios del host y la
// respuesta al reto (el mismo vector fijo que en src-tauri/src/red/sala.rs).
import { describe, expect, it } from "vitest";
import { leerEnlace, urlImagen, urlWebSocket } from "../cliente-web/src/enlace";
import { aplicarCambios, estaCaida, vistaVacia } from "../cliente-web/src/vista";
import { prueba } from "../cliente-web/src/conexion.svelte";

const S = "AbCdEfGhIjKlMnOpQrStUv";
const K = "0123456789abcdef_-XYZ";

describe("enlace", () => {
  it("lee túnel, sala y secreto de lo que va tras el #", () => {
    expect(leerEnlace(`#t=casa-roja.trycloudflare.com&s=${S}&k=${K}`)).toEqual({ tunel: "casa-roja.trycloudflare.com", sala: S, secreto: K });
  });

  it("solo acepta túneles de Cloudflare y claves bien formadas", () => {
    for (const malo of [
      `#t=evil.example.com&s=${S}&k=${K}`,
      `#t=a.trycloudflare.com.evil.com&s=${S}&k=${K}`,
      `#t=a.trycloudflare.com/x&s=${S}&k=${K}`,
      `#t=a.trycloudflare.com&s=corto&k=${K}`,
      `#t=a.trycloudflare.com&s=${S}&k=con espacios aqui!!`,
      `#t=a.trycloudflare.com&s=${S}`,
      ""
    ]) {
      expect(leerEnlace(malo), malo).toBeNull();
    }
  });

  it("construye las URLs del host", () => {
    const e = leerEnlace(`#t=x.trycloudflare.com&s=${S}&k=${K}`)!;
    expect(urlWebSocket(e)).toBe("wss://x.trycloudflare.com/ws");
    expect(urlImagen(e, "SES", "imagenes/a b.png", "ficha")).toBe(
      "https://x.trycloudflare.com/imagen?ruta=imagenes%2Fa+b.png&sesion=SES&mini=ficha"
    );
  });
});

describe("respuesta al reto", () => {
  it("es la misma que calcula Rust (red::sala::prueba)", async () => {
    expect(await prueba("token", "reto", "x.trycloudflare.com")).toBe(
      "193db0e84f792dd66761cc1dc113b7ad299da23603ddaedf9ebb23e7022db4f8"
    );
  });
});

describe("aplicar los cambios del host", () => {
  const inst = (id: number, extra: object = {}) =>
    ({ id_instancia: id, id_mapa: 1, base: { id, nombre: `F${id}`, parametros: [] }, valores_actuales: [], coordenadas: "NoPuesto", rotacion: 0, orden: id, ...extra }) as any;
  const mapa = (id: number) => ({ id }) as any;

  it("sustituye por id, quita lo quitado y deja lo demás", () => {
    let v = { ...vistaVacia(), mapa: mapa(1), instancias: [inst(1), inst(2)] };
    v = aplicarCambios(v, { instancias: [inst(2, { rotacion: 1 }), inst(3)], instancias_quitadas: [1] });
    expect(v.instancias.map((i) => [i.id_instancia, i.rotacion])).toEqual([[2, 1], [3, 0]]);
    expect(v.mapa).toEqual(mapa(1));

    v = aplicarCambios(v, { mapa: { actual: null } });
    expect(v.mapa).toBeNull();
    v = aplicarCambios(v, { mapa: { actual: mapa(2) }, definiciones: [] });
    expect(v.mapa).toEqual(mapa(2));
    expect(aplicarCambios(v, {})).toEqual(v);
  });

  it("una ficha con la vida a 0 se ve en gris", () => {
    const vida = { id: 1, tiene_maximo: true, atenuar_si_negativo: true } as any;
    const conVida = (n: number) => inst(1, { valores_actuales: [{ id_definicion: 1, valor: n }] });
    expect(estaCaida(conVida(0), [vida])).toBe(true);
    expect(estaCaida(conVida(3), [vida])).toBe(false);
    expect(estaCaida(inst(1), [vida]), "sin valor visible, no").toBe(false);
  });
});
