// src/lib/imagenes.ts
//
// En la partida, la imagen de fondo de un mapa se guarda como RUTA RELATIVA al directorio de
// datos de la app (p. ej. "imagenes/ab12...f3.png"), que es lo que devuelve el comando
// `importar_imagen`. Para mostrarla hay que convertirla en una URL que entienda el webview:
// eso es lo único que hace este módulo.
import { invoke, convertFileSrc } from "@tauri-apps/api/core";

let dirDatos = "";

/** Llamar UNA vez al arrancar, antes de pintar ningún mapa. */
export async function iniciarImagenes(): Promise<void> {
  dirDatos = await invoke<string>("obtener_dir_datos");
}

// Valores que ya son una URL completa (mapas creados con versiones anteriores de la app).
const YA_ES_URL = /^(https?:|data:|blob:|asset:)/i;

/** Copia una imagen del sistema a la carpeta de datos de la app. Devuelve la ruta relativa a guardar. */
export async function importarImagen(origen: string): Promise<string> {
  const ruta = await invoke<string>("importar_imagen", { origen });
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

/** Ruta guardada en el mapa -> URL que el <img>/canvas puede cargar. "" si no hay imagen. */
export function urlDeImagen(ref: string | null | undefined): string {
  if (!ref) return "";
  if (YA_ES_URL.test(ref)) return ref;

  // Usamos el mismo separador que el sistema para que la ruta sea coherente (C:\...\imagenes\x.png).
  const sep = dirDatos.includes("\\") ? "\\" : "/";
  return convertFileSrc(`${dirDatos}${sep}${ref.split("/").join(sep)}`);
}
