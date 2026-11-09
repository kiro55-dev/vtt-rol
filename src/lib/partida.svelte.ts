// src/lib/partida.svelte.ts
//
// Estado compartido de la partida en el front: zonas, mapas, mapa abierto, rejilla, banco de
// entidades e instancias. Los componentes lo importan y lo leen directamente
// (`partida.zonas`, `partida.mapaActual`...) y llaman a sus métodos para cambiarlo; ya no hay
// props ni callbacks (`onRecargarZonas`...) que atravesar de padre a hijo.
//
// Rust sigue siendo la fuente de verdad: cada método escribe en Rust y vuelve a leer.
import type { Encuadre } from "$lib/recorteFoto";
import { EVENTO_PARTIDA_CAMBIADA, invoke, listen } from "$lib/backend";
import type { PartesCambiadas } from "$lib/red/replica";
import { iniciarImagenes, importarImagen } from "$lib/imagenes";
import { miniaturas } from "$lib/miniaturas.svelte";
import { enviarAviso, avisoFoco } from "$lib/avisosTablero";
import { desplazarGrupo, encajar, ladosInstancia, rejillaDe } from "$lib/tablero";
import { ajustesVista } from "$lib/ajustesVista.svelte";
import { esJugador } from "$lib/modo";
import { tiradas } from "$lib/tiradas.svelte";

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
  /** Lo ven los jugadores de la sala (la casilla del mapa). Lo cambia solo `compartirMapa`. */
  compartido: boolean;
}

export type TipoEntidad = "Enemigo" | "NPC" | "Jugador";
export type Cuerpo = "Pequenio" | { Grande: number[] };

// ---- Parámetros (espejo de modelos.rs) ----
// Partida: DefinicionParametro (comunes a todas las fichas) · Ficha: Parametro (su valor)
// · Instancia: ValorActual (solo los que tienen máximo, p. ej. la vida)
export type ValorParametro = { Numero: number } | { Texto: string };
export type Visibilidad = "SiemprePublico" | "SiemprePrivado" | "Variable";
/** El array es RGBA */
export type Forma =
  | { Rombo: number[] }
  | { Circulo: number[] }
  | { Cuadrado: number[] }
  | { BarraVida: number[] }
  /** Solo con máximo: «actual/máx» en «Fichas activas», en ese color */
  | { Resumen: number[] };

export interface DefinicionParametro {
  id: number;
  clave: string;
  valor_por_defecto: ValorParametro;
  tiene_maximo: boolean;
  representar_mapa: Forma | null;
  visibilidad: Visibilidad;
  unidad: string | null;
  /** Solo con máximo: si el valor actual llega a 0 o menos, la ficha se ve atenuada (como la vida) */
  atenuar_si_negativo: boolean;
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

/**
 * ¿La instancia trae ese parámetro? Rust no manda a la pantalla los privados (ver `visibilidad.rs`):
 * un parámetro que falta no se enseña, ni su valor, ni su resumen, ni lo que se dibuja con él.
 */
export function veParametro(instancia: Instancia, idDefinicion: number): boolean {
  return instancia.base.parametros.some((p) => p.id_definicion === idDefinicion);
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
  /** Miniatura propia de la ficha (la que encuadró el usuario); la pone Rust, nunca el front. Sin ella se usa la automática de la foto. */
  miniatura_url: string | null;
  /** Encuadre de la foto en la miniatura (zoom 100-400 %, x e y 0-100): se guarda para poder seguir moviéndola al reabrir la ficha. */
  foto_encuadre: Encuadre;
  archivos_adjuntos: (string | null)[];
  es_unico: boolean;
  /** Ids de los jugadores que controlan la ficha (`IdJugador` en Rust), sin el host, que lo puede todo. Vacío = solo el host. */
  propietarios: number[];
}

// Casilla del tablero: (0,0) arriba a la izquierda. La lógica está en $lib/tablero.ts (y en tablero.rs).
export interface Casilla {
  x: number;
  y: number;
}

// En una ficha de varias casillas es la casilla CENTRAL
export type Coordenada = "NoPuesto" | { Casilla: Casilla };

/** Dónde poner una instancia: una casilla de un mapa (null = activa, fuera del tablero). */
export interface Destino {
  id_mapa: number;
  casilla: Casilla;
}

export interface Instancia {
  id_instancia: number;
  id_mapa: number | null;
  base: Entidad;
  valores_actuales: ValorActual[];
  coordenadas: Coordenada;
  activo?: boolean;
  /** Cuartos de vuelta en el sentido del reloj (0..3); con uno impar, ancho y alto se intercambian */
  rotacion: number;
  /** La imagen se dibuja en espejo (izquierda-derecha, en el eje de la propia ficha, antes de girar). Solo aspecto. */
  espejo?: boolean;
  /** Posición en «Fichas activas» */
  orden: number;
  /** Altura en el tablero (la mayor, encima). La pone Rust al poner o mover la ficha. */
  capa?: number;
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
/** Los cambios de valores actuales (vida...) se mandan 1 s después del último. */
const ESPERA_VALORES_MS = 1000;
/** Tope de un valor actual en positivo y negativo (igual que `LIMITE_VALOR_ACTUAL` en Rust). */
export const LIMITE_VALOR_ACTUAL = 16000;
const claveValor = (idInstancia: number, idDefinicion: number) => `${idInstancia}:${idDefinicion}`;

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

/** ¿Está la instancia puesta en el tablero del mapa `idMapa`? (las activas sin colocar no lo están) */
export function estaEnMapa(instancia: Instancia, idMapa: number | null | undefined): boolean {
  return idMapa != null && instancia.id_mapa === idMapa && typeof instancia.coordenadas === "object";
}

/**
 * Orden completo de «Fichas activas» tras reordenar solo las que se ven (con el filtro de mapa puesto):
 * las visibles se reparten, en su orden nuevo, por los huecos que ya ocupaban, y las ocultas no se mueven.
 * `visibles` es la lista nueva de las que se ven (las mismas que había, en otro orden).
 */
export function reordenarConOcultas(todos: readonly number[], visibles: readonly number[]): number[] {
  const esVisible = new Set(visibles);
  let k = 0;
  return todos.map((id) => (esVisible.has(id) ? visibles[k++] : id));
}

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

  /** «Fichas activas» de arriba abajo. */
  instanciasOrdenadas = $derived([...this.instancias].sort((a, b) => a.orden - b.orden || a.id_instancia - b.id_instancia));
  /** Las que enseña «Fichas activas»: todas, o con el filtro «Este mapa» (ver `ajustesVista`) solo las puestas
   *  en el tablero del mapa abierto. A un jugador solo le llega lo que puede ver, así que el filtro es el mismo. */
  instanciasVisibles = $derived(
    ajustesVista.soloMapaActual
      ? this.instanciasOrdenadas.filter((i) => estaEnMapa(i, this.mapaActual?.id))
      : this.instanciasOrdenadas
  );

  /** Fichas con el foco (varias con Ctrl, como en el banco). Solo se guardan los ids: se actualizan solas al recargar
   *  las instancias y desaparecen si Rust las elimina (p. ej. al borrar su zona). */
  idsSeleccionados = $state<number[]>([]);
  seleccionadas = $derived(this.instancias.filter((i) => this.idsSeleccionados.includes(i.id_instancia)));
  /** La ficha con el foco si hay exactamente una (sus parámetros se ven y se pueden cambiar). */
  instanciaSeleccionada = $derived(this.seleccionadas.length === 1 ? this.seleccionadas[0] : null);

  /** Cambios de valores actuales aún sin mandar a Rust, por "idInstancia:idDefinicion" (ver `cambiarValorActual`). */
  /** Fichas que otro jugador tiene con el foco (llega por red como aviso, no se guarda). Se dibujan
   *  con otro contorno; no cambian el foco propio. */
  idsFocoAjeno = $state<number[]>([]);
  /** Último foco propio avisado a los demás, para no repetirlo. */
  #focoAvisado: number[] = [];

  valoresPendientes = $state<Record<string, number>>({});
  #temporizadorValores: ReturnType<typeof setTimeout> | null = null;

  /** Aviso de una línea (algo que falló sin que haya una ventana donde mostrarlo); desaparece solo. */
  avisos = $state("");
  #temporizadorAviso: ReturnType<typeof setTimeout> | null = null;
  avisar(texto: string) {
    this.avisos = texto;
    if (this.#temporizadorAviso) clearTimeout(this.#temporizadorAviso);
    this.#temporizadorAviso = setTimeout(() => (this.avisos = ""), 6000);
  }

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
    this.#escucharCambios();
    await iniciarImagenes(); // hace falta antes de mostrar ningún mapa con imagen
    this.definiciones = await invoke<DefinicionParametro[]>("obtener_definiciones_parametros");
    await this.recargarZonas();
    await this.seleccionarZona(this.zonasOrdenadas[0]?.id ?? null);
    await this.recargarEntidades();
  }

  #escuchando = false;

  /**
   * La partida cambió por algo que no hizo esta pantalla: en el jugador, lo que le envía el host; en el host
   * (fase 2), lo que hagan los jugadores. Se relee lo que cambió, como tras cualquier acción propia.
   */
  #escucharCambios() {
    if (this.#escuchando) return;
    this.#escuchando = true;
    listen<Partial<PartesCambiadas>>(EVENTO_PARTIDA_CAMBIADA, (e) => void this.alCambiarPartida(e.payload)).catch((err) =>
      console.warn("partida-cambiada no disponible:", err)
    );
  }

  /** Relee lo que cambió. Si la zona o el mapa abiertos dejan de existir (o de verse), pasa al primero. */
  async alCambiarPartida(partes: Partial<PartesCambiadas> = { zonas: true, mapas: true, entidades: true, definiciones: true, tiradas: true }) {
    if (partes.tiradas) await tiradas.cargar(); // el chat es de todos: también llega lo que tiran los demás
    if (partes.definiciones) await this.recargarDefiniciones();
    if (partes.zonas || partes.mapas) {
      await this.recargarZonas();
      if (this.zonaId === null || !this.zonas.some((z) => z.id === this.zonaId)) {
        await this.seleccionarZona(this.zonasOrdenadas[0]?.id ?? null);
      } else {
        await this.recargarMapas();
      }
    }
    if (partes.entidades) await this.recargarEntidades();
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
    // Un cambio de rejilla o de valores pendiente se escribiría en la partida nueva
    await this.guardarRejilla();
    await this.guardarValoresPendientes();
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
    this.idsSeleccionados = [];
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
    // Las seleccionadas que ya no existen (borradas, o se fue su zona) dejan de estarlo
    const ids = this.idsBancoSeleccionados.filter((id) => banco.some((e) => e.id === id));
    if (ids.length !== this.idsBancoSeleccionados.length) this.idsBancoSeleccionados = ids;
  }

  // ------------------------------------------------------------------
  // Selección múltiple en el banco (Ctrl+clic). Solo de esta pantalla: no se guarda ni se avisa.
  // ------------------------------------------------------------------

  /** Fichas del banco seleccionadas, en el orden en que se eligieron. */
  idsBancoSeleccionados = $state<number[]>([]);

  alternarSeleccionBanco(id: number) {
    const ids = this.idsBancoSeleccionados;
    this.idsBancoSeleccionados = ids.includes(id) ? ids.filter((x) => x !== id) : [...ids, id];
  }

  limpiarSeleccionBanco() {
    if (this.idsBancoSeleccionados.length > 0) this.idsBancoSeleccionados = [];
  }

  /** Manda a la papelera varias fichas de una vez (un Ctrl+Z las devuelve todas). */
  async borrarEntidades(ids: number[]) {
    if (ids.length === 0) return;
    try {
      await invoke("borrar_entidades", { ids });
    } catch (err) {
      console.error("borrar_entidades:", err);
    }
    await this.recargarEntidades();
    if (this.entidadEnEdicion && ids.includes(this.entidadEnEdicion.id)) this.cerrarEditorEntidad();
    this.idsBancoSeleccionados = this.idsBancoSeleccionados.filter((id) => !ids.includes(id));
  }

  /**
   * Ctrl+Z: devuelve lo último que se borró (fichas, un mapa o una zona). Las creaciones no se
   * deshacen. Devuelve false si la papelera estaba vacía.
   */
  async restaurarPapelera(): Promise<boolean> {
    let r: { zonas: number[]; mapas: number[]; entidades: number[] } | null;
    try {
      r = await invoke("restaurar_papelera");
    } catch (err) {
      console.error("restaurar_papelera:", err);
      return false;
    }
    if (!r) return false;
    await this.recargarZonas();
    if (this.zonaId === null || r.zonas.length > 0) {
      // Se abre la zona recuperada (o, si no había ninguna abierta, la primera)
      await this.seleccionarZona(r.zonas[0] ?? this.zonasOrdenadas[0]?.id ?? null);
    }
    await this.recargarMapas(this.mapaActual === null ? (r.mapas[0] ?? undefined) : undefined);
    await this.recargarEntidades();
    return true;
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

  /**
   * Da el foco a una ficha (`null` = a ninguna). Con `anadir` (Ctrl) la añade o la quita del
   * conjunto en vez de sustituirlo. Los valores cambiados de la que pierde el foco se guardan ya.
   */
  seleccionarInstancia(instancia: Instancia | null, anadir = false) {
    const ids = this.idsSeleccionados;
    let nuevos: number[];
    if (!instancia) nuevos = [];
    else if (!anadir) nuevos = [instancia.id_instancia];
    else if (ids.includes(instancia.id_instancia)) nuevos = ids.filter((id) => id !== instancia.id_instancia);
    else nuevos = [...ids, instancia.id_instancia];

    if (nuevos.length === ids.length && nuevos.every((id, i) => id === ids[i])) return;
    this.#cambiarFoco(nuevos);
  }

  /**
   * Da el foco a varias fichas a la vez (el recuadro de Ctrl+arrastrar en el tablero). Con `anadir` se
   * suman a las que ya lo tenían; si no, las sustituyen.
   */
  seleccionarInstancias(ids: readonly number[], anadir = false) {
    const actuales = this.idsSeleccionados;
    const nuevos = anadir ? [...actuales, ...ids.filter((id) => !actuales.includes(id))] : [...new Set(ids)];
    if (nuevos.length === actuales.length && nuevos.every((id, i) => id === actuales[i])) return;
    this.#cambiarFoco(nuevos);
  }

  // Cambia el foco propio, guarda lo pendiente de las fichas que lo pierden y avisa a los demás.
  #cambiarFoco(nuevos: number[]) {
    this.idsSeleccionados = nuevos;
    void this.guardarValoresPendientes();
    // El foco es solo de este momento: se avisa (por red, cuando la haya) pero no se guarda en disco
    const igual = nuevos.length === this.#focoAvisado.length && nuevos.every((id, i) => id === this.#focoAvisado[i]);
    if (igual) return;
    this.#focoAvisado = [...nuevos];
    enviarAviso(avisoFoco(nuevos));
  }

  /** Lo que otro jugador tiene con el foco ahora (sustituye a lo anterior). */
  recibirFocoAjeno(ids: number[]) {
    this.idsFocoAjeno = [...ids];
  }

  estaSeleccionada(idInstancia: number) {
    return this.idsSeleccionados.includes(idInstancia);
  }

  /**
   * Con una sola ficha con foco, lo pasa a la anterior (-1) o la siguiente (+1) de «Fichas activas» (de las
   * que se ven, con el filtro de mapa puesto). Es circular: tras la última va la primera y antes de la
   * primera, la última. Si la que tiene el foco está oculta por el filtro, empieza por los extremos.
   */
  seleccionarVecina(paso: -1 | 1): boolean {
    const actual = this.instanciaSeleccionada;
    if (!actual || this.idsSeleccionados.length !== 1) return false;
    const lista = this.instanciasVisibles;
    const i = lista.findIndex((x) => x.id_instancia === actual.id_instancia);
    if (i === -1) {
      if (lista.length === 0) return false;
      this.seleccionarInstancia(lista[paso === 1 ? 0 : lista.length - 1]);
      return true;
    }
    if (lista.length < 2) return false;
    this.seleccionarInstancia(lista[(i + paso + lista.length) % lista.length]);
    return true;
  }

  // ------------------------------------------------------------------
  // Valores actuales (vida...): se agrupan antes de mandarlos
  // ------------------------------------------------------------------

  /**
   * La ficha se ve atenuada (gris) si algún parámetro marcado con «atenuar_si_negativo» (la vida de
   * serie lo está) tiene su valor actual en 0 o menos. Lo decide la definición, no ningún id fijo.
   */
  estaCaida(instancia: Instancia): boolean {
    return this.definiciones.some(
      (d) =>
        d.atenuar_si_negativo &&
        d.tiene_maximo &&
        instancia.valores_actuales.some((v) => v.id_definicion === d.id) &&
        this.valorMostrado(instancia, d.id) <= 0
    );
  }

  /** Valor actual que se ve: el de Rust más lo que aún no se ha mandado. */
  valorMostrado(instancia: Instancia, idDefinicion: number): number {
    return valorActual(instancia, idDefinicion) + (this.valoresPendientes[claveValor(instancia.id_instancia, idDefinicion)] ?? 0);
  }

  /**
   * Suma `delta` a un valor con máximo (p. ej. la vida). Puede pasar del máximo (vida extra) o bajar
   * de 0 (vida negativa). Se ve al momento pero
   * no se manda a Rust hasta 1 s después del último cambio (o antes si la ficha pierde el foco):
   * 50 pasos de rueda son UN delta de 50, no 50 deltas.
   */
  cambiarValorActual(idInstancia: number, idDefinicion: number, delta: number) {
    const instancia = this.instancias.find((i) => i.id_instancia === idInstancia);
    if (!instancia || delta === 0 || !this.controla(instancia)) return;
    const clave = claveValor(idInstancia, idDefinicion);
    const actual = valorActual(instancia, idDefinicion);
    // El máximo es solo el valor por defecto: se puede pasar de él o bajar de 0 (como en Rust)
    const nuevo = Math.min(LIMITE_VALOR_ACTUAL, Math.max(-LIMITE_VALOR_ACTUAL, this.valorMostrado(instancia, idDefinicion) + delta));
    if (nuevo === actual) delete this.valoresPendientes[clave];
    else this.valoresPendientes[clave] = nuevo - actual;

    if (this.#temporizadorValores) clearTimeout(this.#temporizadorValores);
    this.#temporizadorValores = setTimeout(() => void this.guardarValoresPendientes(), ESPERA_VALORES_MS);
  }

  /** Manda ya los valores pendientes: uno por cada valor cambiado, con la suma de todos sus pasos. */
  async guardarValoresPendientes() {
    if (this.#temporizadorValores) clearTimeout(this.#temporizadorValores);
    this.#temporizadorValores = null;
    const enviados = Object.entries(this.valoresPendientes).filter(([, delta]) => delta !== 0);
    if (enviados.length === 0) return;

    for (const [clave, delta] of enviados) {
      const [idInstancia, idDefinicion] = clave.split(":").map(Number);
      try {
        await invoke("modificar_valor_actual", { idInstancia, idDefinicion, delta });
      } catch (err) {
        console.error("modificar_valor_actual:", err);
      }
    }
    this.instancias = await invoke<Instancia[]>("obtener_instancias_activas");
    // Lo mandado ya está en el valor de Rust; lo que se haya cambiado mientras tanto sigue pendiente
    for (const [clave, delta] of enviados) {
      const resto = (this.valoresPendientes[clave] ?? 0) - delta;
      if (resto === 0) delete this.valoresPendientes[clave];
      else this.valoresPendientes[clave] = resto;
    }
  }

  // ------------------------------------------------------------------
  // Instancias
  // ------------------------------------------------------------------

  /** Gira la ficha un cuarto de vuelta en el sentido del reloj (Rust la mete en el mapa si no cabe). */
  async rotarInstancia(idInstancia: number) {
    await this.#cambiarInstancias("rotar_instancia", { idInstancia });
  }

  /** Pone o quita el espejo de la imagen de la ficha (Ctrl + clic derecho en el tablero). */
  async espejarInstancia(idInstancia: number) {
    // Se ve al momento; Rust lo confirma al releer
    this.instancias = this.instancias.map((i) => (i.id_instancia === idInstancia ? { ...i, espejo: !i.espejo } : i));
    await this.#cambiarInstancias("espejar_instancia", { idInstancia });
  }

  /** Nuevo orden de «Fichas activas» (todos los ids, de arriba abajo). */
  async reordenarInstancias(ids: number[]) {
    // Se ve al momento; Rust lo confirma al releer
    this.instancias = this.instancias.map((i) => ({ ...i, orden: Math.max(0, ids.indexOf(i.id_instancia)) }));
    await this.#cambiarInstancias("reordenar_instancias", { ids });
  }

  /**
   * Instancia una ficha del banco en una casilla (`destino`) o solo como activa (null). Una ficha
   * única que ya tenía instancia la reutiliza (Rust decide). Soltada en «Fichas activas» queda
   * seleccionada; en el tablero no (así no se dibujan sus formas al soltarla).
   */
  async instanciar(idEntidad: number, destino: Destino | null) {
    const instancia = await this.#cambiarInstancias<Instancia>("instanciar_entidad", { idEntidad, destino });
    if (instancia && destino === null) this.seleccionarInstancia(instancia);
  }

  /**
   * Instancia varias fichas del banco de golpe (selección múltiple). En el tablero la primera cae en
   * `destino` y las demás en cuadrícula a su lado (Rust); con null, todas activas sin colocar.
   * Como con una sola: en «Fichas activas» quedan todas con el foco; en el tablero, ninguna.
   */
  async instanciarVarias(idsEntidades: number[], destino: Destino | null) {
    const creadas = await this.#cambiarInstancias<Instancia[]>("instanciar_entidades", { ids: idsEntidades, destino });
    if (creadas?.length && destino === null) this.#cambiarFoco(creadas.map((i) => i.id_instancia));
  }

  /** Pone una instancia en otra casilla (la casilla final la encaja Rust). */
  async moverInstancia(idInstancia: number, destino: Destino) {
    const rejilla = this.#rejillaDelMapa(destino.id_mapa);
    const inst = this.instancias.find((i) => i.id_instancia === idInstancia);
    if (rejilla && inst) {
      this.#adelantarPosiciones(destino.id_mapa, new Map([[idInstancia, encajar(rejilla, destino.casilla, ladosInstancia(inst))]]));
    }
    await this.#cambiarInstancias("mover_instancia", { idInstancia, destino });
  }

  /**
   * Mueve juntas varias fichas del mapa `idMapa` `(dx, dy)` casillas, conservando sus distancias
   * (las seleccionadas que se arrastran a la vez). Rust las recorta en el borde del mapa.
   */
  async moverInstancias(ids: number[], idMapa: number, dx: number, dy: number) {
    const rejilla = this.#rejillaDelMapa(idMapa);
    const grupo = ids.flatMap((id) => {
      const i = this.instancias.find((x) => x.id_instancia === id);
      return i && typeof i.coordenadas === "object" ? [{ id, casilla: i.coordenadas.Casilla, lados: ladosInstancia(i) }] : [];
    });
    if (rejilla && grupo.length === ids.length) {
      const destinos = desplazarGrupo(rejilla, grupo, dx, dy);
      this.#adelantarPosiciones(idMapa, new Map(grupo.map((f, k) => [f.id, destinos[k]])));
    }
    await this.#cambiarInstancias("mover_instancias", { ids, idMapa, dx, dy });
  }

  /** La saca del tablero; sigue en «Fichas activas». */
  async quitarDelTablero(idInstancia: number) {
    // Se quita ya del tablero (si no, al soltarla se vería un instante donde estaba)
    this.instancias = this.instancias.map((i) =>
      i.id_instancia === idInstancia ? { ...i, id_mapa: null, coordenadas: "NoPuesto" as const } : i
    );
    await this.#cambiarInstancias("mover_instancia", { idInstancia, destino: null });
  }

  /** Rejilla de un mapa con su tamaño guardado: la misma con la que calcula Rust. */
  #rejillaDelMapa(idMapa: number) {
    const mapa = this.mapas.find((m) => m.id === idMapa) ?? (this.mapaActual?.id === idMapa ? this.mapaActual : null);
    return mapa ? rejillaDe(mapa.ancho, mapa.alto, mapa.rejilla) : null;
  }

  /**
   * Pone ya las fichas donde caerán (las mismas cuentas que Rust) y encima de las demás, sin esperar a
   * que Rust conteste: si no, al soltarlas se verían un instante en su casilla de antes. Lo que diga
   * Rust al releer manda.
   */
  #adelantarPosiciones(idMapa: number, casillas: Map<number, Casilla>) {
    let capa = Math.max(-1, ...this.instancias.map((i) => i.capa ?? 0)) + 1;
    // Las del grupo suben en el orden en que estaban apiladas, como en Rust
    const capas = new Map(
      this.instancias
        .filter((i) => casillas.has(i.id_instancia))
        .sort((a, b) => (a.capa ?? 0) - (b.capa ?? 0))
        .map((i) => [i.id_instancia, capa++])
    );
    this.instancias = this.instancias.map((i) => {
      const casilla = casillas.get(i.id_instancia);
      return casilla ? { ...i, id_mapa: idMapa, coordenadas: { Casilla: casilla }, capa: capas.get(i.id_instancia) } : i;
    });
  }

  /** Saca varias del tablero (siguen en «Fichas activas»). */
  async quitarVariasDelTablero(ids: number[]) {
    for (const idInstancia of ids) await this.quitarDelTablero(idInstancia);
  }

  /** Varias dejan de estar activas (Supr, o arrastrarlas juntas al banco). */
  async borrarInstancias(ids: number[]) {
    for (const id of ids) await this.borrarInstancia(id);
  }

  /** Deja de estar activa. */
  async borrarInstancia(idInstancia: number) {
    // Lo pendiente de una ficha que desaparece ya no hace falta
    for (const clave of Object.keys(this.valoresPendientes)) {
      if (clave.startsWith(`${idInstancia}:`)) delete this.valoresPendientes[clave];
    }
    await this.#cambiarInstancias("borrar_instancia", { idInstancia });
    if (this.idsSeleccionados.includes(idInstancia)) {
      this.#cambiarFoco(this.idsSeleccionados.filter((id) => id !== idInstancia));
    }
  }

  // Llama a Rust y vuelve a leer las instancias. Un error (instancia o mapa que ya no existe) se
  // muestra en consola: el arrastre ya terminó y no hay dónde enseñarlo.
  async #cambiarInstancias<T = void>(comando: string, args: Record<string, unknown>): Promise<T | undefined> {
    try {
      return await invoke<T>(comando, args);
    } catch (err) {
      console.error(`${comando}:`, err);
      return undefined;
    } finally {
      this.instancias = await invoke<Instancia[]>("obtener_instancias_activas");
    }
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
        referencia = await importarImagen(rutaOrigen, "mapa");
      } catch (err) {
        console.error("No se pudo importar la imagen:", err);
        return;
      }
    }

    const nuevo = await invoke<Mapa>("crear_mapa", { idZona: this.zonaId, urlImagenFondo: referencia });
    await this.recargarMapas(nuevo.id); // abre el mapa nuevo
  }

  /** La casilla del mapa: lo ven (o dejan de ver) los jugadores de la sala, al momento. */
  async compartirMapa(mapa: Mapa, compartido: boolean) {
    await invoke("compartir_mapa", { id: mapa.id, compartido });
    await this.recargarMapas();
  }

  async renombrarMapa(mapa: Mapa, nombre: string) {
    await this.#editarMapa(mapa.id, { nombre });
  }

  async cambiarImagenMapa(mapa: Mapa, rutaOrigen: string) {
    let referencia: string;
    try {
      referencia = await importarImagen(rutaOrigen, "mapa");
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
   * ¿Puede esta pantalla cambiar la ficha? El host, todas; un jugador, las suyas (su banco solo trae las que le
   * ha dado el host). El host lo vuelve a comprobar con cada acción: esto solo evita ofrecer lo que va a rechazar.
   */
  controla(instancia: Instancia): boolean {
    return !esJugador || this.banco.some((e) => e.id === instancia.base.id);
  }

  /** Sube en cada apertura del editor: la vista lo usa de `{#key}` para crear una ventana nueva aunque pase
   *  directamente de una ficha a otra. */
  sesionEditor = $state(0);

  /** Cómo guardar la ventana abierta (la registra VentanaFicha): devuelve true si se guardó y se cerró. */
  #guardarEditor: (() => Promise<boolean>) | null = null;

  /** La ventana de la ficha dice cómo guardarla. Devuelve cómo dejar de hacerlo. */
  registrarEditor(guardar: () => Promise<boolean>) {
    this.#guardarEditor = guardar;
    return () => {
      if (this.#guardarEditor === guardar) this.#guardarEditor = null;
    };
  }

  /**
   * Abre la ventana de una ficha. Con `entidad` edita una existente (sobre una copia);
   * sin ella crea un borrador: Rust reserva el id real en ese instante, así los archivos
   * que se adjunten ya viven en su carpeta definitiva (archivos/entidad_<id>).
   *
   * Si ya hay otra ficha abierta, primero se GUARDA (como al pulsar Guardar) y la ventana pasa a la nueva.
   * Si no se puede guardar (falta el nombre, Rust la rechaza...) la abierta se queda con el motivo a la vista
   * y no se abre nada. Pedir la que ya está abierta no hace nada. Devuelve true si se abrió la pedida.
   */
  async abrirEditorEntidad(entidad?: Entidad | null, fotoUrl: string | null = null): Promise<boolean> {
    if (this.#abriendo) return false;
    const abierta = this.entidadEnEdicion;
    if (abierta && entidad && !this.editorEsBorrador && abierta.id === entidad.id) return false;
    this.#abriendo = true;
    try {
      if (abierta && !(await this.#guardarEditor?.())) return false;
      this.errorEditor = "";
      if (entidad) {
        // Clonamos para no mutar el estado en vivo hasta pulsar Guardar. Se parte de la del banco por si
        // es la ficha que se acaba de guardar
        const actual = this.banco.find((e) => e.id === entidad.id) ?? entidad;
        this.entidadEnEdicion = structuredClone($state.snapshot(actual));
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
      this.sesionEditor++;
      return true;
    } catch (err) {
      console.error("No se pudo abrir la ficha:", err);
      this.avisar(`No se pudo abrir la ficha: ${err instanceof Error ? err.message : String(err)}`);
      return false;
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
