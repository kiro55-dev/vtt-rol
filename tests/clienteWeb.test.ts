// Pruebas de lo que usa el navegador del jugador (src/lib/red): el enlace, la réplica de su vista (aplicar
// los cambios del host y contestar con ella los comandos de la app) y la respuesta al reto (el mismo vector
// fijo que en src-tauri/src/red/sala.rs).
import { describe, expect, it } from "vitest";
import { leerEnlace, urlImagen, urlWebSocket } from "$lib/red/enlace";
import { aplicarCambios, NO_DISPONIBLE, partesCambiadas, responder, rutaMiniatura, separarMiniatura, vistaVacia } from "$lib/red/replica";
import { prueba } from "$lib/red/conexion.svelte";

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

const inst = (id: number, extra: object = {}) =>
  ({ id_instancia: id, id_mapa: 1, base: { id, nombre: `F${id}`, parametros: [] }, valores_actuales: [], coordenadas: "NoPuesto", rotacion: 0, orden: id, ...extra }) as any;
const mapa = (id: number, id_zona = 1) => ({ id, id_zona, nombre: `M${id}` }) as any;
const zona = (id: number) => ({ id, nombre: `Z${id}`, orden: 0 }) as any;

describe("réplica: aplicar los cambios del host", () => {
  it("sustituye por id, quita lo quitado y deja lo demás", () => {
    let v = { ...vistaVacia(), zonas: [zona(1)], mapas: [mapa(1)], instancias: [inst(1), inst(2)] };
    v = aplicarCambios(v, { instancias: [inst(2, { rotacion: 1 }), inst(3)], instancias_quitadas: [1] });
    expect(v.instancias.map((i) => [i.id_instancia, i.rotacion])).toEqual([[2, 1], [3, 0]]);

    v = aplicarCambios(v, { mapas: [mapa(2, 2)], mapas_quitados: [1], zonas: [zona(2)], zonas_quitadas: [1] });
    expect(v.mapas.map((m) => m.id)).toEqual([2]);
    expect(v.zonas.map((z) => z.id)).toEqual([2]);
    expect(aplicarCambios(v, {})).toEqual(v);
  });

  it("dice qué partes de la app tienen que releer", () => {
    expect(partesCambiadas({ instancias_quitadas: [1] })).toEqual({ zonas: false, mapas: false, entidades: true, definiciones: false });
    expect(partesCambiadas({ mapas: [mapa(1)], definiciones: [] })).toEqual({ zonas: false, mapas: true, entidades: false, definiciones: true });
    expect(partesCambiadas({ zonas_quitadas: [3], entidades: [] })).toEqual({ zonas: true, mapas: false, entidades: false, definiciones: false });
  });
});

describe("réplica: contesta los comandos de la app como lo haría Rust", () => {
  const vista = { zonas: [zona(1), zona(2)], mapas: [mapa(1, 1), mapa(2, 2), mapa(3, 1)], instancias: [inst(5)], entidades: [{ id: 9 }] as any, definiciones: [{ id: 1 }] as any };

  it("las lecturas salen de la réplica (copias, para que nadie la cambie sin querer)", () => {
    expect(responder("obtener_zonas", {}, vista)).toEqual(vista.zonas);
    expect((responder("obtener_mapas_zona", { zonaId: 1 }, vista) as any[]).map((m) => m.id)).toEqual([1, 3]);
    expect(responder("obtener_banco_entidades", {}, vista)).toEqual(vista.entidades);
    expect(responder("obtener_instancias_activas", {}, vista)).toEqual(vista.instancias);
    expect(responder("obtener_definiciones_parametros", {}, vista)).toEqual(vista.definiciones);
    expect(responder("obtener_historial_tiradas", {}, vista)).toEqual([]);
    expect(responder("obtener_zonas", {}, vista)).not.toBe(vista.zonas);
  });

  it("las miniaturas las genera el host: la ruta lleva la marca y la URL la pide con mini", () => {
    const r = responder("obtener_miniatura", { ruta: "imagenes/a.png", tipo: "mapa" }, vista) as string;
    expect(r).toBe(rutaMiniatura("imagenes/a.png", "mapa"));
    expect(separarMiniatura(r)).toEqual({ ruta: "imagenes/a.png", mini: "mapa" });
    expect(separarMiniatura("imagenes/b.png")).toEqual({ ruta: "imagenes/b.png" });
  });

  it("lo que cambia la partida aún no lo puede hacer un jugador", () => {
    for (const cmd of ["crear_zona", "mover_instancia", "borrar_entidad", "compartir_mapa", "tirar_dado"]) {
      expect(() => responder(cmd, {}, vista), cmd).toThrow(NO_DISPONIBLE);
    }
    expect(responder("avisar_tablero", {}, vista)).toBeNull();
  });
});
