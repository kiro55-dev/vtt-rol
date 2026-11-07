// Pruebas de la lógica de la ventana de ficha (src/lib/editorFicha.svelte.ts) con un Rust falso.
import { describe, expect, it, vi } from "vitest";

type Args = Record<string, any>;

const DIR = "archivos/entidad_7/";
let llamadas: { cmd: string; args: Args }[] = [];
let respuestas: Record<string, (a: Args) => any> = {};
let partidaFalsa: any;

const llamadasA = (cmd: string) => llamadas.filter((l) => l.cmd === cmd).map((l) => l.args);

// Los mismos que `parametros_por_defecto()` en Rust
const def = (id: number, clave: string, n: number, visibilidad: string, extra: Record<string, any> = {}) => ({
  id, clave, valor_por_defecto: { Numero: n }, tiene_maximo: false, representar_mapa: null, visibilidad, unidad: null, ...extra
});
const DEFINICIONES = [
  def(1, "Vida", 10, "Variable", { tiene_maximo: true }),
  def(2, "Armadura", 10, "Variable"),
  def(3, "Velocidad", 6, "SiemprePublico", { representar_mapa: { Rombo: [59, 130, 246, 90] }, unidad: "m" }),
  def(4, "Rango arma", 1, "SiemprePublico", { representar_mapa: { Circulo: [239, 68, 68, 90] }, unidad: "m" })
];

function entidad(extra: Record<string, any> = {}) {
  return {
    id: 7, nombre: "Orco", tipo: "Enemigo",
    parametros: [{ id_definicion: 1, valor: { Numero: 18 }, publico: true }],
    cuerpo: "Pequenio", zona_id: 1, foto_url: null, archivos_adjuntos: [], es_unico: false, ...extra
  } as any;
}

async function nuevoEditor(e = entidad(), esBorrador = false) {
  vi.resetModules();
  llamadas = [];
  respuestas = {
    leer_archivo_texto: (a) => `disco:${a.rutaRelativa}`,
    crear_documento_texto: () => ({ ruta: DIR + "Documento_1.txt" }),
    importar_archivo: (a) => ({ ruta: DIR + a.origen.split("/").pop() }),
    renombrar_archivo_adjunto: (a) => ({ ruta: DIR + a.nuevoNombre }),
    importar_imagen: (a) => "imagenes/" + a.origen.split("/").pop()
  };
  vi.doMock("@tauri-apps/api/core", () => ({
    invoke: async (cmd: string, args: Args = {}) => {
      llamadas.push({ cmd, args: structuredClone(args) });
      return respuestas[cmd]?.(args);
    },
    convertFileSrc: (p: string) => p
  }));
  partidaFalsa = {
    errorEditor: "",
    definiciones: structuredClone(DEFINICIONES),
    entidadEnEdicion: e,
    guardarEntidad: vi.fn(async () => true),
    cancelarEditorEntidad: vi.fn(async () => {}),
    borrarEntidad: vi.fn(async () => {
      partidaFalsa.entidadEnEdicion = null;
    })
  };
  vi.doMock("$lib/partida.svelte", () => ({ partida: partidaFalsa }));
  const { EditorFicha } = await import("$lib/editorFicha.svelte");
  return new EditorFicha(e, esBorrador);
}

describe("textos adjuntos", () => {
  it("al guardar solo escribe los .txt modificados que siguen adjuntos", async () => {
    const ed = await nuevoEditor(entidad({ archivos_adjuntos: [DIR + "a.txt", DIR + "b.txt", DIR + "c.txt"] }));
    await ed.abrirPestana(0); // leído pero sin tocar: no se reescribe
    await ed.abrirPestana(1);
    ed.escribir("nuevo b");
    await ed.abrirPestana(2);
    ed.escribir("nuevo c");
    ed.quitar(2); // modificado pero quitado: tampoco

    expect(await ed.guardar()).toBe(true);
    expect(llamadasA("guardar_archivo_texto").map((a) => [a.adjunto.ruta, a.contenido])).toEqual([
      [DIR + "b.txt", "nuevo b"]
    ]);
    expect(partidaFalsa.guardarEntidad).toHaveBeenCalledOnce();
  });

  it("un .txt que no se pudo leer no se puede editar (así no se pisa el archivo)", async () => {
    const ed = await nuevoEditor(entidad({ archivos_adjuntos: [DIR + "a.txt"] }));
    respuestas.leer_archivo_texto = () => {
      throw "sin permiso";
    };
    await ed.abrirPestana(0);
    ed.escribir("esto no debe llegar al disco");

    expect(ed.lecturaFallida(DIR + "a.txt")).toBe(true);
    expect(ed.error).toContain("sin permiso");
    await ed.guardar();
    expect(llamadasA("guardar_archivo_texto")).toEqual([]);
  });

  it("solo lee cada .txt una vez", async () => {
    const ed = await nuevoEditor(entidad({ archivos_adjuntos: [DIR + "a.txt", DIR + "b.pdf"] }));
    await ed.abrirPestana(0);
    await ed.abrirPestana(1);
    await ed.abrirPestana(0);
    expect(llamadasA("leer_archivo_texto")).toHaveLength(1);
    expect(ed.textos[DIR + "a.txt"]).toBe("disco:" + DIR + "a.txt");
  });

  it("al quitar una pestaña anterior a la activa, la activa sigue siendo el mismo archivo", async () => {
    const ed = await nuevoEditor(entidad({ archivos_adjuntos: [DIR + "a.txt", DIR + "b.txt", DIR + "c.txt"] }));
    await ed.abrirPestana(2);
    ed.quitar(0);
    expect(ed.pestana).toBe(1);
    expect(ed.rutaActiva).toBe(DIR + "c.txt");
    ed.quitar(1);
    expect(ed.rutaActiva).toBe(DIR + "b.txt");
  });
});

describe("renombrar", () => {
  it("un archivo ya guardado se copia (la copia es de la sesión); uno de la sesión se sustituye", async () => {
    const ed = await nuevoEditor(entidad({ archivos_adjuntos: [DIR + "viejo.txt"] }));
    await ed.renombrar(0, "nuevo.txt");
    await ed.crearTexto();
    await ed.renombrar(1, "notas.txt");

    expect(ed.adjuntos).toEqual([DIR + "nuevo.txt", DIR + "notas.txt"]);
    await ed.cancelar();
    expect(partidaFalsa.cancelarEditorEntidad).toHaveBeenCalledWith([DIR + "nuevo.txt", DIR + "notas.txt"]);
  });

  it("el texto editado se guarda con la ruta nueva", async () => {
    const ed = await nuevoEditor(entidad({ archivos_adjuntos: [DIR + "a.txt"] }));
    await ed.abrirPestana(0);
    ed.escribir("hola");
    await ed.renombrar(0, "b.txt");
    await ed.guardar();
    expect(llamadasA("guardar_archivo_texto").map((a) => [a.adjunto.ruta, a.contenido])).toEqual([
      [DIR + "b.txt", "hola"]
    ]);
  });

  it("no llama a Rust si el nombre está vacío o no cambia", async () => {
    const ed = await nuevoEditor(entidad({ archivos_adjuntos: [DIR + "a.txt"] }));
    await ed.renombrar(0, "  ");
    await ed.renombrar(0, "a.txt");
    expect(llamadasA("renombrar_archivo_adjunto")).toEqual([]);
  });
});

describe("guardar, cancelar y borrar", () => {
  it("al guardar descarta lo creado en la sesión (Rust conserva lo que se use)", async () => {
    const ed = await nuevoEditor(entidad({ foto_url: "imagenes/1.png" }), true);
    await ed.ponerFoto("/fotos/2.png");
    await ed.importarArchivos(["/docs/ficha.pdf"]);

    expect(ed.entidad.foto_url).toBe("imagenes/2.png");
    expect(ed.rutaActiva).toBe(DIR + "ficha.pdf");
    expect(await ed.guardar()).toBe(true);
    expect(llamadasA("descartar_archivos_borrador")).toEqual([
      { rutas: ["imagenes/1.png", "imagenes/2.png", DIR + "ficha.pdf"] }
    ]);
  });

  it("si Rust rechaza la ficha no se descarta nada", async () => {
    const ed = await nuevoEditor(entidad(), true);
    await ed.crearTexto();
    partidaFalsa.guardarEntidad.mockResolvedValueOnce(false);
    expect(await ed.guardar()).toBe(false);
    expect(llamadasA("descartar_archivos_borrador")).toEqual([]);
  });

  it("una ficha no válida no llega a Rust", async () => {
    const ed = await nuevoEditor(entidad({ nombre: "  ", archivos_adjuntos: [DIR + "a.txt"] }));
    await ed.abrirPestana(0);
    ed.escribir("x");
    expect(await ed.guardar()).toBe(false);
    expect(ed.error).toBe("La ficha necesita un nombre");

    ed.entidad.nombre = "Orco";
    ed.ponerNumero(1, "abc" as any); // lo que deja un input numérico raro
    await ed.guardar();
    expect(ed.error).toBe("Vida: debe ser un número entero");
    expect(llamadasA("guardar_archivo_texto")).toEqual([]);
    expect(partidaFalsa.guardarEntidad).not.toHaveBeenCalled();
  });

  it("borrar descarta lo de la sesión solo si la ficha se borró", async () => {
    const ed = await nuevoEditor();
    await ed.crearTexto();
    await ed.borrar();
    expect(partidaFalsa.borrarEntidad).toHaveBeenCalledOnce();
    expect(llamadasA("descartar_archivos_borrador")).toEqual([{ rutas: [DIR + "Documento_1.txt"] }]);
  });
});

describe("parámetros", () => {
  it("rellena los que faltan con su valor por defecto y en el orden de las definiciones", async () => {
    const ed = await nuevoEditor();
    expect(ed.entidad.parametros).toEqual([
      { id_definicion: 1, valor: { Numero: 18 }, publico: true },
      { id_definicion: 2, valor: { Numero: 10 }, publico: false },
      { id_definicion: 3, valor: { Numero: 6 }, publico: true }, // visibilidad fija: siempre público
      { id_definicion: 4, valor: { Numero: 1 }, publico: true }
    ]);
  });

  it("sin definiciones (no se pudieron leer) no toca los parámetros de la ficha", async () => {
    vi.resetModules();
    const e = entidad();
    vi.doMock("$lib/partida.svelte", () => ({ partida: { definiciones: [] } }));
    const { EditorFicha } = await import("$lib/editorFicha.svelte");
    new EditorFicha(e, false);
    expect(e.parametros).toHaveLength(1);
  });

  it("agrupa los numéricos de dos en dos y deja aparte los de texto", async () => {
    const ed = await nuevoEditor();
    expect(ed.campos.numericos.map((par) => par.map((d) => d.clave))).toEqual([
      ["Vida", "Armadura"],
      ["Velocidad", "Rango arma"]
    ]);
    expect(ed.campos.textos).toEqual([]);
  });

  it("lee y escribe valores y visibilidad por id de definición", async () => {
    const ed = await nuevoEditor();
    ed.ponerNumero(2, 4);
    ed.ponerPublico(2, true);
    expect([ed.numero(2), ed.esPublico(2), ed.numero(99)]).toEqual([4, true, 0]);
    expect(await ed.guardar()).toBe(true);
    expect(partidaFalsa.guardarEntidad.mock.calls[0][0].parametros[1]).toEqual({
      id_definicion: 2, valor: { Numero: 4 }, publico: true
    });
  });
});

describe("cuerpo", () => {
  it("1×1 es Pequenio, lo demás Grande, y los valores se acotan a 1..255", async () => {
    const ed = await nuevoEditor();
    ed.cuerpoX = 3;
    expect(ed.entidad.cuerpo).toEqual({ Grande: [3, 1] });
    ed.cuerpoY = 999;
    expect(ed.entidad.cuerpo).toEqual({ Grande: [3, 255] });
    ed.cuerpoX = 1;
    ed.cuerpoY = null as any; // input vacío
    expect(ed.entidad.cuerpo).toBe("Pequenio");
    expect([ed.cuerpoX, ed.cuerpoY]).toEqual([1, 1]);
  });
});
