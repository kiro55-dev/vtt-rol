// Pruebas de lo que usa el navegador del jugador (src/lib/red): el enlace, la réplica de su vista (aplicar
// los cambios del host y contestar con ella los comandos de la app) y la respuesta al reto (el mismo vector
// fijo que en src-tauri/src/red/sala.rs).
import { describe, expect, it } from "vitest";
import { leerEnlace, urlImagen, urlWebSocket } from "$lib/red/enlace";
import { aplicarCambios, NO_DISPONIBLE, PETICIONES, partesCambiadas, responder, rutaMiniatura, separarMiniatura, vistaVacia } from "$lib/red/replica";
import { prueba } from "$lib/red/conexion.svelte";

const S = "AbCdEfGhIjKlMnOpQrStUv";
const K = "0123456789abcdef_-XYZ";

describe("enlace", () => {
  it("lee túnel, sala y secreto de lo que va tras el #", () => {
    expect(leerEnlace(`#t=casa-roja.trycloudflare.com&s=${S}&k=${K}`)).toEqual({
      tunel: "casa-roja.trycloudflare.com",
      sala: S,
      secreto: K,
      mismoOrigen: false,
      seguro: true
    });
  });

  it("sin `t`, el host es quien sirve la página (dirección propia o red local), con su esquema", () => {
    const propia = leerEnlace(`#s=${S}&k=${K}`, { host: "kirem-2.tail12739.ts.net", protocol: "https:" })!;
    expect(propia).toEqual({ tunel: "kirem-2.tail12739.ts.net", sala: S, secreto: K, mismoOrigen: true, seguro: true });
    expect(urlWebSocket(propia)).toBe("wss://kirem-2.tail12739.ts.net/ws");
    expect(urlImagen(propia, "SES", "imagenes/a.png")).toBe("/imagen?ruta=imagenes%2Fa.png&sesion=SES");

    const lan = leerEnlace(`#s=${S}&k=${K}`, { host: "26.1.2.3:8080", protocol: "http:" })!;
    expect(lan).toMatchObject({ tunel: "26.1.2.3:8080", mismoOrigen: true, seguro: false });
    expect(urlWebSocket(lan)).toBe("ws://26.1.2.3:8080/ws");

    expect(leerEnlace(`#s=${S}&k=${K}`), "sin saber dónde está la página").toBeNull();
    expect(leerEnlace(`#s=${S}&k=${K}`, { host: "x.es", protocol: "file:" })).toBeNull();
  });

  it("acepta la dirección propia del host (dominio y puerto opcional)", () => {
    expect(leerEnlace(`#t=mesa.mi-dominio.es&s=${S}&k=${K}`)?.tunel).toBe("mesa.mi-dominio.es");
    expect(leerEnlace(`#t=mesa.mi-dominio.es:8443&s=${S}&k=${K}`)?.tunel).toBe("mesa.mi-dominio.es:8443");
    expect(leerEnlace(`#t=Mesa.MI-dominio.es&s=${S}&k=${K}`)?.tunel, "en minúsculas").toBe("mesa.mi-dominio.es");
  });

  it("solo acepta una dirección sin rutas ni usuarios, y claves bien formadas", () => {
    for (const malo of [
      `#t=a.trycloudflare.com/x&s=${S}&k=${K}`,
      `#t=usuario@a.trycloudflare.com&s=${S}&k=${K}`,
      `#t=a b.es&s=${S}&k=${K}`,
      `#t=-malo.es&s=${S}&k=${K}`,
      `#t=a.es:puerto&s=${S}&k=${K}`,
      `#t=javascript:alert(1)&s=${S}&k=${K}`,
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
    expect(partesCambiadas({ instancias_quitadas: [1] })).toEqual({ zonas: false, mapas: false, entidades: true, definiciones: false, tiradas: false });
    expect(partesCambiadas({ mapas: [mapa(1)], definiciones: [] })).toEqual({ zonas: false, mapas: true, entidades: false, definiciones: true, tiradas: false });
    expect(partesCambiadas({ zonas_quitadas: [3], entidades: [] })).toEqual({ zonas: true, mapas: false, entidades: false, definiciones: false, tiradas: false });
    expect(partesCambiadas({ historial_tiradas: [] })).toEqual({ zonas: false, mapas: false, entidades: false, definiciones: false, tiradas: true });
    expect(partesCambiadas({ mapa_visible_cambiado: true, mapa_visible: 3 })).toEqual({ zonas: false, mapas: true, entidades: false, definiciones: false, tiradas: false });
  });

  it("el mapa que debe ver el jugador es el del host: llega por los cambios, y puede quedarse en ninguno", () => {
    let v = aplicarCambios(vistaVacia(), { mapa_visible_cambiado: true, mapa_visible: 4 });
    expect(v.mapa_visible).toBe(4);
    v = aplicarCambios(v, { zonas: [zona(1)] }); // otros cambios no lo tocan
    expect(v.mapa_visible).toBe(4);
    v = aplicarCambios(v, { mapa_visible_cambiado: true }); // ya no hay ninguno
    expect(v.mapa_visible).toBeNull();
  });
});

describe("réplica: contesta los comandos de la app como lo haría Rust", () => {
  const chat = [{ total: 9, comando: "1d10 (9)", hora: "10:00", jugador: 1, autor: "Ana" }];
  const vista = { zonas: [zona(1), zona(2)], mapas: [mapa(1, 1), mapa(2, 2), mapa(3, 1)], instancias: [inst(5)], entidades: [{ id: 9 }] as any, definiciones: [{ id: 1 }] as any, historial_tiradas: chat };

  it("obtener_mapa_visible da el mapa del host (de cualquier zona) o, si aún no hay, el primero que ve", () => {
    expect((responder("obtener_mapa_visible", {}, { ...vista, mapa_visible: 2 }) as any).id).toBe(2);
    expect((responder("obtener_mapa_visible", {}, { ...vista, mapa_visible: null }) as any).id).toBe(1);
    expect((responder("obtener_mapa_visible", {}, { ...vista, mapa_visible: 99 }) as any).id).toBe(1);
    expect(responder("obtener_mapa_visible", {}, { ...vista, mapas: [] })).toBeNull();
  });

  it("las lecturas salen de la réplica (copias, para que nadie la cambie sin querer)", () => {
    expect(responder("obtener_zonas", {}, vista)).toEqual(vista.zonas);
    expect((responder("obtener_mapas_zona", { zonaId: 1 }, vista) as any[]).map((m) => m.id)).toEqual([1, 3]);
    expect(responder("obtener_banco_entidades", {}, vista)).toEqual(vista.entidades);
    expect(responder("obtener_instancias_activas", {}, vista)).toEqual(vista.instancias);
    expect(responder("obtener_definiciones_parametros", {}, vista)).toEqual(vista.definiciones);
    expect(responder("obtener_historial_tiradas", {}, vista)).toEqual(chat);
    expect(responder("obtener_historial_personal", {}, vista, ["1d6", "2d8"])).toEqual(["1d6", "2d8"]);
    expect(responder("obtener_zonas", {}, vista)).not.toBe(vista.zonas);
  });

  it("las miniaturas las genera el host: la ruta lleva la marca y la URL la pide con mini", () => {
    const r = responder("obtener_miniatura", { ruta: "imagenes/a.png", tipo: "mapa" }, vista) as string;
    expect(r).toBe(rutaMiniatura("imagenes/a.png", "mapa"));
    expect(separarMiniatura(r)).toEqual({ ruta: "imagenes/a.png", mini: "mapa" });
    expect(separarMiniatura("imagenes/b.png")).toEqual({ ruta: "imagenes/b.png" });
  });

  it("lo que no está en la lista blanca de acciones lo rechaza (y las acciones permitidas no las contesta la réplica)", () => {
    for (const cmd of ["crear_zona", "borrar_entidad", "compartir_mapa", "limpiar_chat", "reordenar_instancias", "importar_archivo"]) {
      expect(PETICIONES.has(cmd), cmd).toBe(false);
      expect(() => responder(cmd, {}, vista), cmd).toThrow(NO_DISPONIBLE);
    }
    for (const cmd of ["tirar_dado", "mover_instancia", "modificar_entidad", "avisar_tablero"]) {
      expect(PETICIONES.has(cmd), cmd).toBe(true);
      expect(() => responder(cmd, {}, vista), cmd).toThrow(NO_DISPONIBLE); // se piden al host, no se leen de aquí
    }
  });
});
