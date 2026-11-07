// src/lib/partida.svelte.ts
//
// Estado compartido de la partida en el front: zonas, mapas, mapa abierto, rejilla, banco de
// entidades e instancias. Los componentes lo importan y lo leen directamente
// (`partida.zonas`, `partida.mapaActual`...) y llaman a sus métodos para cambiarlo; ya no hay
// props ni callbacks (`onRecargarZonas`...) que atravesar de padre a hijo.
//
// Rust sigue siendo la fuente de verdad: cada método escribe en Rust y vuelve a leer.
import { invoke } from "@tauri-apps/api/core";
import { iniciarImagenes, importarImagen } from "$lib/imagenes";
import { miniaturas } from "$lib/miniaturas.svelte";

export interface Zona {
  id: number;
  nombre: string;
  orden: number;
}

export interface Mapa {
  id: number;
  id_zona: number;
  nombre: string;
  url_imagen_fondo: string | null; // ruta relativa devuelta por importar_imagen
  ancho: number;
  alto: number;
  rejilla: number;
  color_opacidad: number[]; // [r, g, b, a] con a en 0..255
  grosor: number;
  orden: number;
}

export type TipoEntidad = "Enemigo" | "NPC" | "Jugador";
export type Cuerpo = "Pequenio" | { Grande: number[] };

// ---- Parámetros (espejo de modelos.rs) ----
// Partida: DefinicionParametro (comunes a todas las fichas) · Ficha: Parametro (su valor)
// · Instancia: ValorActual (solo los que tienen máximo, p. ej. la vida)
export type ValorParametro = { Numero: number } | { Texto: string };
export type Visibilidad = "SiemprePublico" | "SiemprePrivado" | "Variable";
/** El array es RGBA */
export type Forma = { Rombo: number[] } | { Circulo: number[] } | { Cuadrado: number[] } | { BarraVida: number[] };

export interface DefinicionParametro {
  id: number;
  clave: string;
  valor_por_defecto: ValorParametro;
  tiene_maximo: boolean;
  representar_mapa: Forma | null;
  visibilidad: Visibilidad;
  unidad: string | null;
}

export interface Parametro {
  id_definicion: number;
  valor: ValorParametro;
  publico: boolean;
}

export interface ValorActual {
  id_definicion: number;
  valor: number;
}

/** Ids de los parámetros de serie (fijos en Rust: `ID_VIDA`...). */
export const ID_PARAMETRO = { vida: 1, armadura: 2, velocidad: 3, rangoArma: 4 } as const;

/** Valor numérico de un parámetro de una ficha (0 si no lo tiene o es de texto). */
export function numeroDe(entidad: Entidad, idDefinicion: number): number {
  const v = entidad.parametros.find((p) => p.id_definicion === idDefinicion)?.valor;
  return v && "Numero" in v ? v.Numero : 0;
}

/** Valor actual de un parámetro con máximo en una instancia (si no lo lleva, el de la ficha). */
export function valorActual(instancia: Instancia, idDefinicion: number): number {
  return (
    instancia.valores_actuales.find((v) => v.id_definicion === idDefinicion)?.valor ??
    numeroDe(instancia.base, idDefinicion)
  );
}

export interface Entidad {
  id: number;
  nombre: string;
  tipo: TipoEntidad;
  /** Uno por definición, en el orden de las definiciones */
  parametros: Parametro[];
  cuerpo: Cuerpo;
  zona_id: number;
  foto_url: string | null;
  archivos_adjuntos: (string | null)[];
  es_unico: boolean;
}

export interface Instancia {
  id_instancia: number;
  id_mapa: number | null;
  base: Entidad;
  valores_actuales: ValorActual[];
  coordenadas: "NoPuesto" | { Localizacion: number[] };
  activo?: boolean;
}

export interface Rejilla {
  visible: boolean;
  tamano: number;
  color: string; // "#rrggbb"
  grosor: number;
  opacidad: number; // 0..1
}

export const LIMITES_REJILLA = { tamanoMin: 5, tamanoMax: 300, grosorMin: 0, grosorMax: 10 } as const;

const REJILLA_DEFECTO: Rejilla = { visible: true, tamano: 50, color: "#ffffff", grosor: 1, opacidad: 0.3 };

// Los cambios de rejilla se agrupan: se guardan 5 s después del último, o antes si hace falta.
const ESPERA_GUARDADO_MS = 5000;

const porOrden = (a: { orden?: number }, b: { orden?: number }) => (a.orden ?? 0) - (b.orden ?? 0);

export function hexARgba(hex: string, opacidad: number): number[] {
  const limpio = hex.replace("#", "");
  const completo = limpio.length === 3 ? limpio.split("").map((x) => x + x).join("") : limpio;
  const n = parseInt(completo, 16) || 0;
  const a = Math.round(Math.min(1, Math.max(0, opacidad)) * 255);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255, a];
}

export const rgbAHex = (r: number, g: number, b: number) =>
  "#" + [r, g, b].map((v) => Number(v).toString(16).padStart(2, "0")).join("");

/** Elemento que queda al borrar `id` de una lista ordenada: el siguiente, o si no el anterior. */
function vecinoTrasBorrar<T extends { id: number }>(ordenada: T[], id: number): T | null {
  const i = ordenada.findIndex((x) => x.id === id);
  if (i === -1) return null;
  return ordenada[i + 1] ?? ordenada[i - 1] ?? null;
}

class Partida {
  zonas = $state<Zona[]>([]);
  /** Mapas de la zona seleccionada. */
  mapas = $state<Mapa[]>([]);
  banco = $state<Entidad[]>([]);
  instancias = $state<Instancia[]>([]);
  /** Parámetros comunes a todas las fichas (vida, armadura...). */
  definiciones = $state<DefinicionParametro[]>([]);

  /** Solo se guarda el id: la ficha seleccionada se actualiza sola al recargar las instancias,
   *  y desaparece si Rust la elimina (p. ej. al borrar su zona). */
  idInstanciaSeleccionada = $state<number | null>(null);
  instanciaSeleccionada = $derived(
    this.instancias.find((i) => i.id_instancia === this.idInstanciaSeleccionada) ?? null
  );

  /** `null` = no hay ninguna zona (todavía no se ha creado, o se borró la última). */
  zonaId = $state<number | null>(null);
  mapaActual = $state<Mapa | null>(null);
  rejilla = $state<Rejilla>({ ...REJILLA_DEFECTO });

  zonasOrdenadas = $derived([...this.zonas].sort(porOrden));
  mapasOrdenados = $derived([...this.mapas].sort(porOrden));

  #temporizador: ReturnType<typeof setTimeout> | null = null;
  #pendiente = false;

  // ------------------------------------------------------------------
  // Carga
  // ------------------------------------------------------------------

  async iniciar() {
    await iniciarImagenes(); // hace falta antes de mostrar ningún mapa con imagen
    this.definiciones = await invoke<DefinicionParametro[]>("obtener_definiciones_parametros");
    await this.recargarZonas();
    await this.seleccionarZona(this.zonasOrdenadas[0]?.id ?? null);
    await this.recargarEntidades();
  }

  // ------------------------------------------------------------------
  // Partidas (una carpeta cada una)
  // ------------------------------------------------------------------

  /** Partida abierta ("" hasta que se lee) y todas las que hay. */
  partidas = $state<{ actual: string; lista: string[] }>({ actual: "", lista: [] });

  async recargarPartidas() {
    const { actual, partidas } = await invoke<{ actual: string; partidas: string[] }>("obtener_partidas");
    this.partidas = { actual, lista: partidas };
  }

  /** Guarda la partida abierta y abre otra. Los errores de Rust (no se pudo guardar...) se propagan,
   *  y en ese caso nada cambia. Quien llama debe recargar también las tiradas (`tiradas.cargar()`). */
  cambiarPartida(nombre: string) {
    return this.#alternar("abrir_partida", nombre);
  }

  /** Crea una partida vacía (con los parámetros de serie) y pasa a ella. */
  crearPartida(nombre: string) {
    return this.#alternar("crear_partida", nombre);
  }

  async #alternar(cmd: string, nombre: string) {
    // Un cambio de rejilla pendiente (se guarda 5 s después) se escribiría en la partida nueva
    await this.guardarRejilla();
    try {
      await invoke(cmd, { nombre });
    } finally {
      // Si falló, la lista puede haber cambiado (p. ej. se creó la carpeta pero no se pudo abrir)
      await this.recargarPartidas().catch(() => {});
    }

    // Nada de lo de la partida anterior debe quedar a la vista, ni siquiera un instante
    this.zonas = [];
    this.mapas = [];
    this.banco = [];
    this.instancias = [];
    this.zonaId = null;
    this.mapaActual = null;
    this.idInstanciaSeleccionada = null;
    this.rejilla = { ...REJILLA_DEFECTO };
    miniaturas.limpiar(); // sus URLs llevan dentro la carpeta de la partida anterior
    await this.iniciar();
  }

  async recargarZonas() {
    this.zonas = await invoke<Zona[]>("obtener_zonas");
  }

  /**
   * Vuelve a leer los mapas de la zona seleccionada y abre uno.
   * `preferido`: id del mapa a abrir (por defecto, el que ya estaba abierto). Si no existe, el primero.
   */
  async recargarMapas(preferido?: number | null) {
    await this.guardarRejilla(); // que no se pierdan cambios de rejilla en curso al refrescar
    const quiero = preferido === undefined ? (this.mapaActual?.id ?? null) : preferido;

    if (this.zonaId === null) {
      this.mapas = [];
      this.#abrirMapa(null);
      return;
    }
    this.mapas = await invoke<Mapa[]>("obtener_mapas_zona", { zonaId: this.zonaId });
    const ordenados = this.mapasOrdenados;
    this.#abrirMapa(ordenados.find((m) => m.id === quiero) ?? ordenados[0] ?? null);
  }

  // ------------------------------------------------------------------
  // Parámetros de la partida (comunes a todas las fichas)
  // ------------------------------------------------------------------

  async recargarDefiniciones() {
    this.definiciones = await invoke<DefinicionParametro[]>("obtener_definiciones_parametros");
  }

  /** Crear, borrar o reordenar un parámetro cambia también las fichas: se vuelve a leer todo.
   *  Los errores de Rust (nombre repetido...) se propagan para que los muestre quien llama. */
  async #cambiarDefiniciones(cmd: string, args: Record<string, unknown>) {
    try {
      await invoke(cmd, args);
    } finally {
      await Promise.all([this.recargarDefiniciones(), this.recargarEntidades()]);
    }
  }

  crearDefinicion(definicion: Omit<DefinicionParametro, "id">) {
    return this.#cambiarDefiniciones("crear_definicion_parametro", { definicion: { ...definicion, id: 0 } });
  }

  /** Cambia la configuración de un parámetro (no su tipo). Las fichas conservan sus valores. */
  modificarDefinicion(definicion: DefinicionParametro) {
    return this.#cambiarDefiniciones("modificar_definicion_parametro", { definicion });
  }

  /** Desaparece de la lista y del valor de todas las fichas. */
  borrarDefinicion(id: number) {
    return this.#cambiarDefiniciones("borrar_definicion_parametro", { id });
  }

  /** `ids` = todos los parámetros en el orden nuevo (el que seguirán las fichas). */
  reordenarDefiniciones(ids: number[]) {
    return this.#cambiarDefiniciones("reordenar_definiciones_parametros", { ids });
  }

  /** Rust también cambia el banco y las instancias al borrar zonas o mapas: se vuelven a leer. */
  async recargarEntidades() {
    const [banco, instancias] = await Promise.all([
      invoke<Entidad[]>("obtener_banco_entidades"),
      invoke<Instancia[]>("obtener_instancias_activas")
    ]);
    this.banco = banco;
    this.instancias = instancias;
  }

  // ------------------------------------------------------------------
  // Selección
  // ------------------------------------------------------------------

  async seleccionarZona(id: number | null) {
    if (id === this.zonaId) return;
    this.#desenfocar();
    await this.guardarRejilla(); // cambios pendientes del mapa que se abandona
    this.zonaId = id;
    await this.recargarMapas(null);
  }

  async seleccionarMapa(mapa: Mapa | null) {
    if (!mapa || mapa.id === this.mapaActual?.id) return;
    this.#desenfocar();
    await this.guardarRejilla();
    this.#abrirMapa(mapa);
  }

  // ------------------------------------------------------------------
  // Fichas (instancias activas)
  // ------------------------------------------------------------------

  seleccionarInstancia(instancia: Instancia | null) {
    this.idInstanciaSeleccionada = instancia?.id_instancia ?? null;
  }

  /** PLACEHOLDER: de momento solo cambia el valor en memoria. Cuando exista el comando de Rust
   *  (con el delta `ValorActualModificado`), se llama aquí y los componentes no cambian. */
  modificarValorActual(idInstancia: number, idDefinicion: number, delta: number) {
    const instancia = this.instancias.find((i) => i.id_instancia === idInstancia);
    if (!instancia) return;
    const actual = instancia.valores_actuales.find((v) => v.id_definicion === idDefinicion);
    if (actual) actual.valor += delta;
    else instancia.valores_actuales.push({ id_definicion: idDefinicion, valor: valorActual(instancia, idDefinicion) + delta });
  }

  // ------------------------------------------------------------------
  // Zonas
  // ------------------------------------------------------------------

  async crearZona() {
    const zona = await invoke<Zona>("crear_zona");
    await this.recargarZonas();
    await this.seleccionarZona(zona.id);
  }

  async renombrarZona(zona: Zona, nombre: string) {
    await invoke("modificar_zona", { zona: { ...zona, nombre } });
    await this.recargarZonas();
  }

  /** `ids` en el orden nuevo. Solo se escriben las zonas cuya posición ha cambiado. */
  async reordenarZonas(ids: number[]) {
    for (let i = 0; i < ids.length; i++) {
      const zona = this.zonas.find((z) => z.id === ids[i]);
      if (zona && zona.orden !== i) {
        await invoke("modificar_zona", { zona: { ...zona, orden: i } });
      }
    }
    await this.recargarZonas();
  }

  async borrarZona(zona: Zona) {
    const eraLaActual = zona.id === this.zonaId;
    const siguiente = vecinoTrasBorrar(this.zonasOrdenadas, zona.id);

    // Los cambios de rejilla pendientes serían de un mapa que va a desaparecer: se descartan.
    if (eraLaActual) this.#descartarRejillaPendiente();

    await invoke("eliminar_zona", { zona });
    await this.recargarZonas();
    await this.recargarEntidades(); // Rust quita/desactiva fichas y entidades de esa zona

    if (eraLaActual) {
      // Pasamos a la zona vecina (o a ninguna si era la última): sin esto la pantalla seguía
      // mostrando los mapas de una zona que ya no existe.
      await this.seleccionarZona(siguiente?.id ?? null);
    }
  }

  // ------------------------------------------------------------------
  // Mapas
  // ------------------------------------------------------------------

  /** `rutaOrigen`: imagen del sistema para el fondo (opcional). Se copia a la carpeta de datos. */
  async crearMapa(rutaOrigen: string | null = null) {
    if (this.zonaId === null) return;

    let referencia: string | null = null;
    if (rutaOrigen) {
      try {
        referencia = await importarImagen(rutaOrigen);
      } catch (err) {
        console.error("No se pudo importar la imagen:", err);
        return;
      }
    }

    const nuevo = await invoke<Mapa>("crear_mapa", { idZona: this.zonaId, urlImagenFondo: referencia });
    await this.recargarMapas(nuevo.id); // abre el mapa nuevo
  }

  async renombrarMapa(mapa: Mapa, nombre: string) {
    await this.#editarMapa(mapa.id, { nombre });
  }

  async cambiarImagenMapa(mapa: Mapa, rutaOrigen: string) {
    let referencia: string;
    try {
      referencia = await importarImagen(rutaOrigen);
    } catch (err) {
      console.error("No se pudo importar la imagen:", err);
      return;
    }
    await this.#editarMapa(mapa.id, { url_imagen_fondo: referencia });
  }

  async reordenarMapas(ids: number[]) {
    await this.guardarRejilla();
    for (let i = 0; i < ids.length; i++) {
      const mapa = this.mapas.find((m) => m.id === ids[i]);
      if (mapa && mapa.orden !== i) {
        await invoke("modificar_mapa", { mapa: { ...mapa, orden: i } });
      }
    }
    await this.recargarMapas();
  }

  async borrarMapa(mapa: Mapa) {
    const eraElActual = mapa.id === this.mapaActual?.id;
    const siguiente = vecinoTrasBorrar(this.mapasOrdenados, mapa.id);

    if (eraElActual) this.#descartarRejillaPendiente();

    await invoke("borrar_mapa", { mapa });
    await this.recargarMapas(eraElActual ? (siguiente?.id ?? null) : undefined);
    await this.recargarEntidades(); // las fichas que estaban en ese mapa cambian
  }

  /**
   * Cambia campos de un mapa. Primero guarda la rejilla pendiente y parte de la copia MÁS
   * RECIENTE del mapa, para no pisar con datos viejos lo que acaba de guardarse.
   */
  async #editarMapa(id: number, cambios: Partial<Mapa>) {
    await this.guardarRejilla();
    const fresco = this.mapas.find((m) => m.id === id);
    if (!fresco) return;
    await invoke("modificar_mapa", { mapa: { ...fresco, ...cambios } });
    await this.recargarMapas();
  }

  // ------------------------------------------------------------------
  // Rejilla del mapa abierto (la barra de herramientas llama a estos dos)
  // ------------------------------------------------------------------

  marcarRejillaModificada() {
    if (!this.mapaActual) return;
    this.#pendiente = true;
    if (this.#temporizador) clearTimeout(this.#temporizador);
    this.#temporizador = setTimeout(() => void this.guardarRejilla(), ESPERA_GUARDADO_MS);
  }

  /** Guarda ya los cambios de rejilla pendientes (si los hay). */
  async guardarRejilla() {
    const base = this.mapaActual;
    if (!this.#pendiente || !base?.id) return;
    this.#descartarRejillaPendiente();

    const { tamanoMin, tamanoMax, grosorMin, grosorMax } = LIMITES_REJILLA;
    const tamano = Math.round(Number(this.rejilla.tamano));
    const grosor = Math.round(Number(this.rejilla.grosor));

    // Un campo vacío o a medias no debe llegar a Rust como 0: se conserva el valor anterior.
    const mapaModificado: Mapa = {
      ...base,
      rejilla: tamano >= tamanoMin && tamano <= tamanoMax ? tamano : base.rejilla,
      grosor: grosor >= grosorMin && grosor <= grosorMax ? grosor : base.grosor,
      color_opacidad: hexARgba(this.rejilla.color, this.rejilla.opacidad)
    };

    try {
      await invoke("modificar_mapa", { mapa: mapaModificado });
    } catch (err) {
      console.error("Error al persistir cambios de rejilla en Rust:", err);
      return;
    }

    if (this.mapaActual?.id === mapaModificado.id) this.mapaActual = mapaModificado;
    const i = this.mapas.findIndex((m) => m.id === mapaModificado.id);
    if (i !== -1) this.mapas[i] = mapaModificado;
  }

  // ------------------------------------------------------------------
  // Internos
  // ------------------------------------------------------------------

  #abrirMapa(mapa: Mapa | null) {
    this.mapaActual = mapa;
    if (!mapa) return;

    this.rejilla.tamano = mapa.rejilla;
    this.rejilla.grosor = mapa.grosor;
    if (Array.isArray(mapa.color_opacidad) && mapa.color_opacidad.length >= 4) {
      const [r, g, b, a] = mapa.color_opacidad;
      this.rejilla.color = rgbAHex(r, g, b);
      this.rejilla.opacidad = Number(a) / 255;
    }
  }

  #descartarRejillaPendiente() {
    if (this.#temporizador) clearTimeout(this.#temporizador);
    this.#temporizador = null;
    this.#pendiente = false;
  }

  /** Quita el foco de un input en edición para que dispare su `change` antes de guardar. */
  #desenfocar() {
    if (typeof document !== "undefined" && document.activeElement instanceof HTMLElement) {
      document.activeElement.blur();
    }
  }

  /** Ficha que se está editando en la ventana flotante (null = ventana cerrada) */
  entidadEnEdicion = $state<Entidad | null>(null);

  // ------------------------------------------------------------------
  // Ventana flotante y persistencia de Entidades
  // ------------------------------------------------------------------

  /** true mientras se edita una ficha NUEVA: ya tiene su id real (reservado en Rust) pero aún no está guardada */
  editorEsBorrador = $state(false);
  /** Mensaje de error de la última operación del editor (se muestra en la ventana) */
  errorEditor = $state("");

  /**
   * Abre la ventana de una ficha. Con `entidad` edita una existente (sobre una copia);
   * sin ella crea un borrador: Rust reserva el id real en ese instante, así los archivos
   * que se adjunten ya viven en su carpeta definitiva (archivos/entidad_<id>).
   * Si ya hay una ventana abierta se ignora la petición (hay que guardar o cancelar antes).
   */
  async abrirEditorEntidad(entidad?: Entidad | null, fotoUrl: string | null = null) {
    if (this.entidadEnEdicion || this.#abriendo) return;
    this.#abriendo = true;
    this.errorEditor = "";
    try {
      if (entidad) {
        // Clonamos para no mutar el estado en vivo hasta pulsar Guardar
        this.entidadEnEdicion = structuredClone($state.snapshot(entidad));
        this.editorEsBorrador = false;
      } else {
        const zonaDefecto = this.zonaId ?? this.zonasOrdenadas[0]?.id ?? 1;
        const borrador = await invoke<Entidad>("iniciar_borrador_entidad", {
          zonaId: zonaDefecto,
          fotoUrl
        });
        this.entidadEnEdicion = borrador;
        this.editorEsBorrador = true;
      }
    } catch (err) {
      console.error("No se pudo abrir la ficha:", err);
    } finally {
      this.#abriendo = false;
    }
  }
  #abriendo = false;

  /** Cierra la ventana sin guardar. `rutas` = archivos/imágenes que se crearon en esta sesión. */
  async cancelarEditorEntidad(rutas: string[] = []) {
    const e = this.entidadEnEdicion;
    if (!e) return;
    const eraBorrador = this.editorEsBorrador;
    this.entidadEnEdicion = null;
    this.editorEsBorrador = false;
    this.errorEditor = "";
    try {
      if (eraBorrador) {
        await invoke("cancelar_borrador_entidad", { id: e.id, rutas });
      } else if (rutas.length > 0) {
        await invoke("descartar_archivos_borrador", { rutas });
      }
    } catch (err) {
      console.error("Error al descartar el borrador:", err);
    }
  }

  /** Cierra la ventana tras guardar (no descarta nada) */
  cerrarEditorEntidad() {
    this.entidadEnEdicion = null;
    this.editorEsBorrador = false;
    this.errorEditor = "";
  }

  /** Guarda la ficha. Devuelve true si Rust la aceptó; si no, deja el motivo en `errorEditor`. */
  async guardarEntidad(entidad: Entidad): Promise<boolean> {
    this.errorEditor = "";
    try {
      const plano = $state.snapshot(entidad) as Entidad;
      if (this.editorEsBorrador) {
        await invoke<Entidad>("crear_entidad", { entidad: plano });
      } else {
        await invoke("modificar_entidad", { entidad: plano });
      }
      await this.recargarEntidades();
      this.cerrarEditorEntidad();
      return true;
    } catch (err) {
      this.errorEditor = String(err);
      return false;
    }
  }

  async borrarEntidad(entidad: Entidad) {
    try {
      await invoke("borrar_entidad", { entidad: $state.snapshot(entidad) });
      await this.recargarEntidades();
      if (this.entidadEnEdicion?.id === entidad.id) {
        this.cerrarEditorEntidad();
      }
    } catch (err) {
      this.errorEditor = String(err);
    }
  }

}

export const partida = new Partida();
