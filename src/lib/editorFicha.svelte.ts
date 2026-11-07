// src/lib/editorFicha.svelte.ts
//
// Lógica de la ventana de edición de una ficha (VentanaFicha.svelte), separada de la vista para
// poder probarla sin abrir la app.
//
// Archivos "de la sesión": todo lo que se crea o importa mientras la ventana está abierta (la foto
// con la que nace un borrador, los .txt nuevos, los adjuntos arrastrados, las copias que deja un
// renombrado...). Al cancelar se descartan; al guardar se descarta lo que al final no se usó.
// Rust nunca borra lo que la partida guardada referencia, así que pasarle de más no es peligroso.
import { invoke } from "@tauri-apps/api/core";
import { partida, type DefinicionParametro, type Entidad, type Parametro } from "$lib/partida.svelte";
import { importarImagen } from "$lib/imagenes";

interface ArchivoAdjunto {
  ruta: string;
  entidad_archivo: Entidad;
}

const MAX_I16 = 32767;
const MAX_CUERPO = 255;

export const nombreArchivo = (ruta: string) => ruta.split(/[/\\]/).pop() ?? ruta;
export const esTexto = (ruta: string | null | undefined) => !!ruta && ruta.toLowerCase().endsWith(".txt");

/** Entero dentro de [min, max]; lo que no es un número cuenta como `min`. */
export function acotar(v: unknown, min: number, max: number): number {
  const n = Math.round(Number(v));
  return Number.isFinite(n) ? Math.min(max, Math.max(min, n)) : min;
}

/** Número entero dentro del rango de i16 que usa Rust, o un error con el motivo. */
function entero(v: unknown, nombre: string, min = 0): number {
  const n = typeof v === "number" ? v : Number(v);
  if (v === null || v === "" || !Number.isInteger(n)) throw new Error(`${nombre}: debe ser un número entero`);
  if (n < min) throw new Error(`${nombre}: no puede ser menor que ${min}`);
  if (n > MAX_I16) throw new Error(`${nombre}: no puede pasar de ${MAX_I16}`);
  return n;
}

const esNumerico = (d: DefinicionParametro) => "Numero" in d.valor_por_defecto;

/**
 * Igual que `normalizar_parametros` en Rust: un parámetro por definición y en su orden, con el
 * valor por defecto en los que falten, y `publico` según la visibilidad si esta es fija.
 */
export function normalizarParametros(defs: DefinicionParametro[], parametros: Parametro[]): Parametro[] {
  return defs.map((d) => {
    const p = parametros.find((x) => x.id_definicion === d.id) ?? {
      id_definicion: d.id,
      valor: structuredClone(d.valor_por_defecto),
      publico: false
    };
    if (d.visibilidad === "SiemprePublico") p.publico = true;
    else if (d.visibilidad === "SiemprePrivado") p.publico = false;
    return p;
  });
}

export class EditorFicha {
  /** La ficha que se edita (en la app, el mismo objeto reactivo que `partida.entidadEnEdicion`). */
  readonly entidad: Entidad;
  readonly esBorrador: boolean;
  readonly definiciones: DefinicionParametro[];

  /** true mientras se guarda o se borra: los botones se desactivan y no se puede cancelar. */
  guardando = $state(false);
  #error = $state("");

  /** Índice de la pestaña de adjuntos abierta. */
  pestana = $state(0);
  /** Contenido de los .txt ya leídos, por ruta. Sin entrada = todavía no se ha leído. */
  textos = $state<Record<string, string>>({});
  #fallidos = $state<string[]>([]);
  /** Solo estos .txt se escriben al guardar. */
  #modificados = new Set<string>();
  #sesion: string[] = [];

  constructor(entidad: Entidad, esBorrador: boolean, definiciones: DefinicionParametro[] = partida.definiciones) {
    this.entidad = entidad;
    this.esBorrador = esBorrador;
    this.definiciones = definiciones;
    // Sin definiciones (no se pudieron leer) no se toca nada, para no perder los valores de la ficha
    if (definiciones.length > 0) entidad.parametros = normalizarParametros(definiciones, entidad.parametros ?? []);
    // La foto con la que nace un borrador (ficha creada arrastrando una imagen) es de esta sesión
    if (esBorrador && entidad.foto_url) this.#sesion.push(entidad.foto_url);
  }

  // ------------------------------------------------------------------
  // Parámetros (vida, armadura... los define la partida)
  // ------------------------------------------------------------------

  /** Cómo los pinta la ventana: los numéricos de dos en dos y los de texto de uno en uno. */
  get campos(): { numericos: DefinicionParametro[][]; textos: DefinicionParametro[] } {
    const nums = this.definiciones.filter(esNumerico);
    const numericos: DefinicionParametro[][] = [];
    for (let i = 0; i < nums.length; i += 2) numericos.push(nums.slice(i, i + 2));
    return { numericos, textos: this.definiciones.filter((d) => !esNumerico(d)) };
  }

  #parametro(id: number): Parametro | undefined {
    return this.entidad.parametros.find((p) => p.id_definicion === id);
  }

  numero(id: number): number {
    const v = this.#parametro(id)?.valor;
    return v && "Numero" in v ? v.Numero : 0;
  }
  /** Guarda lo que haya en el campo tal cual (aunque esté vacío): `guardar` lo valida. */
  ponerNumero(id: number, n: number) {
    const p = this.#parametro(id);
    if (p) p.valor = { Numero: n };
  }

  texto(id: number): string {
    const v = this.#parametro(id)?.valor;
    return v && "Texto" in v ? v.Texto : "";
  }
  ponerTexto(id: number, t: string) {
    const p = this.#parametro(id);
    if (p) p.valor = { Texto: t };
  }

  esPublico(id: number): boolean {
    return this.#parametro(id)?.publico ?? false;
  }
  ponerPublico(id: number, publico: boolean) {
    const p = this.#parametro(id);
    if (p) p.publico = publico;
  }

  /** Último error de la ventana (validación, archivos) o de Rust al guardar o borrar. */
  get error() {
    return this.#error || partida.errorEditor;
  }

  get adjuntos() {
    return this.entidad.archivos_adjuntos;
  }

  get rutaActiva(): string | null {
    return this.adjuntos[this.pestana] ?? null;
  }

  lecturaFallida(ruta: string) {
    return this.#fallidos.includes(ruta);
  }

  #plano(): Entidad {
    return $state.snapshot(this.entidad) as Entidad;
  }

  // ------------------------------------------------------------------
  // Cuerpo: 1×1 se guarda como "Pequenio", lo demás como { Grande: [x, y] }
  // ------------------------------------------------------------------

  get cuerpoX(): number {
    const c = this.entidad.cuerpo;
    return typeof c === "object" ? (c.Grande[0] ?? 1) : 1;
  }
  set cuerpoX(v: number) {
    this.#ponerCuerpo(v, this.cuerpoY);
  }

  get cuerpoY(): number {
    const c = this.entidad.cuerpo;
    return typeof c === "object" ? (c.Grande[1] ?? 1) : 1;
  }
  set cuerpoY(v: number) {
    this.#ponerCuerpo(this.cuerpoX, v);
  }

  #ponerCuerpo(x: unknown, y: unknown) {
    const ax = acotar(x, 1, MAX_CUERPO);
    const ay = acotar(y, 1, MAX_CUERPO);
    this.entidad.cuerpo = ax === 1 && ay === 1 ? "Pequenio" : { Grande: [ax, ay] };
  }

  // ------------------------------------------------------------------
  // Adjuntos
  // ------------------------------------------------------------------

  /** Abre una pestaña y, si es un .txt que aún no se ha leído, lo lee. */
  async abrirPestana(i: number) {
    if (this.adjuntos.length === 0) return;
    this.pestana = i;
    const ruta = this.adjuntos[i];
    if (!ruta || !esTexto(ruta) || ruta in this.textos) return;

    this.#fallidos = this.#fallidos.filter((r) => r !== ruta);
    try {
      const contenido = await invoke<string>("leer_archivo_texto", { rutaRelativa: ruta });
      // Si entretanto se quitó o se renombró, no se deja una entrada huérfana
      if (this.adjuntos.includes(ruta)) this.textos[ruta] ??= contenido;
    } catch (err) {
      this.#fallidos.push(ruta);
      this.#error = `No se pudo leer «${nombreArchivo(ruta)}»: ${err}`;
    }
  }

  /** Texto escrito en el .txt abierto. Se ignora si aún no se ha leído (no se pisa el archivo). */
  escribir(texto: string) {
    const ruta = this.rutaActiva;
    if (!ruta || !(ruta in this.textos)) return;
    this.textos[ruta] = texto;
    this.#modificados.add(ruta);
  }

  #anadir(ruta: string) {
    this.adjuntos.push(ruta);
    this.#sesion.push(ruta);
  }

  async crearTexto() {
    try {
      const { ruta } = await invoke<ArchivoAdjunto>("crear_documento_texto", { entidad: this.#plano() });
      this.#anadir(ruta);
      this.textos[ruta] = ""; // ya existe vacío en disco: no hace falta leerlo
      this.pestana = this.adjuntos.length - 1;
    } catch (err) {
      this.#error = `No se pudo crear el documento: ${err}`;
    }
  }

  /** Copia archivos del sistema a la carpeta de la ficha y abre el último. */
  async importarArchivos(origenes: string[]) {
    const antes = this.adjuntos.length;
    for (const origen of origenes) {
      try {
        const { ruta } = await invoke<ArchivoAdjunto>("importar_archivo", {
          adjunto: { ruta: "", entidad_archivo: this.#plano() },
          origen
        });
        this.#anadir(ruta);
      } catch (err) {
        this.#error = `No se pudo importar «${origen}»: ${err}`;
      }
    }
    if (this.adjuntos.length > antes) await this.abrirPestana(this.adjuntos.length - 1);
  }

  async renombrar(i: number, nombre: string) {
    const vieja = this.adjuntos[i];
    nombre = nombre.trim();
    if (!vieja || !nombre || nombre === nombreArchivo(vieja)) return;

    try {
      const { ruta } = await invoke<ArchivoAdjunto>("renombrar_archivo_adjunto", {
        adjunto: { ruta: vieja, entidad_archivo: this.#plano() },
        nuevoNombre: nombre
      });

      const k = this.adjuntos.indexOf(vieja);
      if (k >= 0) this.adjuntos[k] = ruta;

      // Si el original es de la ficha ya guardada, Rust lo COPIÓ (no se pierde si se cancela):
      // la copia es un archivo nuevo de la sesión. Si era de la sesión, se movió: se sustituye.
      const s = this.#sesion.indexOf(vieja);
      if (s >= 0) this.#sesion[s] = ruta;
      else this.#sesion.push(ruta);

      if (vieja in this.textos) {
        this.textos[ruta] = this.textos[vieja];
        delete this.textos[vieja];
      }
      if (this.#modificados.delete(vieja)) this.#modificados.add(ruta);
    } catch (err) {
      this.#error = `No se pudo renombrar: ${err}`;
    }
  }

  /** Quita un adjunto de la ficha (el archivo se libera al guardar, o nunca si se cancela). */
  quitar(i: number) {
    const [ruta] = this.adjuntos.splice(i, 1);
    if (ruta) {
      delete this.textos[ruta];
      this.#modificados.delete(ruta);
    }
    // Si se quita una pestaña anterior a la activa, la activa sigue siendo el mismo archivo
    if (i < this.pestana) this.pestana -= 1;
    this.pestana = Math.max(0, Math.min(this.pestana, this.adjuntos.length - 1));
    void this.abrirPestana(this.pestana);
  }

  async abrirEnSistema(ruta: string) {
    try {
      await invoke("abrir_archivo_en_sistema", { rutaRelativa: ruta });
    } catch (err) {
      this.#error = String(err);
    }
  }

  async ponerFoto(origen: string) {
    try {
      const nueva = await importarImagen(origen);
      this.entidad.foto_url = nueva;
      if (!this.#sesion.includes(nueva)) this.#sesion.push(nueva);
    } catch (err) {
      this.#error = `No se pudo importar la imagen: ${err}`;
    }
  }

  // ------------------------------------------------------------------
  // Guardar / cancelar / borrar
  // ------------------------------------------------------------------

  #validar() {
    const e = this.entidad;
    if (!e.nombre?.trim()) throw new Error("La ficha necesita un nombre");
    for (const d of this.definiciones.filter(esNumerico)) {
      this.ponerNumero(d.id, entero(this.numero(d.id), d.clave));
    }
    this.#ponerCuerpo(this.cuerpoX, this.cuerpoY);
  }

  async #descartarSesion() {
    if (this.#sesion.length === 0) return;
    try {
      await invoke("descartar_archivos_borrador", { rutas: [...this.#sesion] });
    } catch (err) {
      console.error("Error al descartar archivos de la sesión:", err);
    }
  }

  /**
   * Valida, escribe los .txt modificados y guarda la ficha. Si algo falla la ventana sigue
   * abierta con el motivo a la vista y no se descarta nada. Devuelve true si se guardó.
   */
  async guardar(): Promise<boolean> {
    if (this.guardando) return false;
    this.guardando = true;
    this.#error = "";
    try {
      this.#validar();

      for (const ruta of this.#modificados) {
        await invoke("guardar_archivo_texto", {
          adjunto: { ruta, entidad_archivo: this.#plano() },
          contenido: this.textos[ruta]
        });
        this.#modificados.delete(ruta);
      }

      if (!(await partida.guardarEntidad(this.entidad))) return false;
      // Fotos/archivos de la sesión que al final no se usaron (cambió de foto, quitó un adjunto...)
      await this.#descartarSesion();
      return true;
    } catch (err) {
      this.#error = err instanceof Error ? err.message : String(err);
      return false;
    } finally {
      this.guardando = false;
    }
  }

  /** Cierra sin guardar. Rust descarta lo creado en la sesión (y, si era un borrador, su carpeta y su id). */
  async cancelar() {
    if (this.guardando) return;
    await partida.cancelarEditorEntidad([...this.#sesion]);
  }

  async borrar() {
    if (this.guardando) return;
    this.guardando = true;
    try {
      await partida.borrarEntidad(this.entidad);
      // Solo si se borró de verdad (si falla, la ventana sigue abierta con el error)
      if (!partida.entidadEnEdicion) await this.#descartarSesion();
    } finally {
      this.guardando = false;
    }
  }
}
