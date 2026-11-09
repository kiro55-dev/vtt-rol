// src/lib/imagenes.ts
//
// En la partida, la imagen de fondo de un mapa se guarda como RUTA RELATIVA al directorio de
// datos de la app (p. ej. "imagenes/ab12...f3.png"), que es lo que devuelve el comando
// `importar_imagen`. Para mostrarla hay que convertirla en una URL que entienda el webview:
// eso es lo único que hace este módulo.
import { invoke, convertFileSrc } from "$lib/backend";
import { esJugador } from "$lib/modo";
import { conexion } from "$lib/red/conexion.svelte";

/** Para qué se importa una imagen: decide qué formatos valen (igual que `imagenes::Uso` en Rust). */
export type UsoImagen = "mapa" | "ficha";

/**
 * Formatos por uso.
 *  - mapa: el tablero se divide en casillas según el tamaño en píxeles de la imagen, así que solo
 *    valen los que se pueden medir (sin SVG ni AVIF).
 *  - ficha: la foto va en un contenedor que ya fija el tamaño, así que valen todos.
 */
export const EXTENSIONES_IMAGEN: Record<UsoImagen, string[]> = {
  mapa: ["png", "jpg", "jpeg", "webp", "gif", "bmp"],
  ficha: ["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif", "svg"]
};

const extension = (ruta: string) => /\.([^./\\]+)$/.exec(ruta.trim())?.[1].toLowerCase() ?? "";

/** ¿Es un formato admitido para ese uso? */
export function esImagen(ruta: string, uso: UsoImagen): boolean {
  return EXTENSIONES_IMAGEN[uso].includes(extension(ruta));
}

let dirDatos = "";

/** Llamar UNA vez al arrancar, antes de pintar ningún mapa. */
export async function iniciarImagenes(): Promise<void> {
  if (esJugador) return; // las imágenes del jugador las sirve el host (ver `urlDeImagen`)
  dirDatos = await invoke<string>("obtener_dir_datos");
}

// Valores que ya son una URL completa (mapas creados con versiones anteriores de la app).
const YA_ES_URL = /^(https?:|data:|blob:|asset:)/i;

/** Copia una imagen del sistema a la carpeta de datos de la app. Devuelve la ruta relativa a guardar. */
export async function importarImagen(origen: string, uso: UsoImagen): Promise<string> {
  // Se rechaza aquí, antes de copiar nada: Rust lo vuelve a comprobar
  if (!esImagen(origen, uso)) {
    throw `Formato de imagen no admitido para ${uso === "mapa" ? "un mapa" : "una ficha"}: ${origen}`;
  }
  const ruta = await invoke<string>("importar_imagen", { origen, uso });
  // Si esta imagen se borró antes y vuelve a importarse, su miniatura se borró con ella:
  // quien tenga cacheada su miniatura debe olvidarla para pedirla de nuevo.
  for (const cb of alImportar) cb(ruta);
  return ruta;
}

const alImportar: Array<(ruta: string) => void> = [];
/** Registra un aviso que se ejecuta cada vez que se importa una imagen. */
export function alImportarImagen(cb: (ruta: string) => void) {
  alImportar.push(cb);
}

/**
 * Ruta guardada en el mapa -> URL que el <img>/canvas puede cargar. "" si no hay imagen.
 * En el host es el archivo de su disco; en el jugador, la misma ruta pedida al host por el túnel
 * (`/imagen`, que solo la da si ese jugador la ve).
 */
export function urlDeImagen(ref: string | null | undefined): string {
  if (!ref) return "";
  if (esJugador) return conexion?.urlImagen(ref) ?? "";
  if (YA_ES_URL.test(ref)) return ref;

  // Usamos el mismo separador que el sistema para que la ruta sea coherente (C:\...\imagenes\x.png).
  const sep = dirDatos.includes("\\") ? "\\" : "/";
  return convertFileSrc(`${dirDatos}${sep}${ref.split("/").join(sep)}`);
}
