// src/lib/miniaturas.svelte.ts
//
// Miniaturas de mapas y fichas. Las genera Rust UNA vez (WebP al 90 %) y las guarda en
// <datos>/miniaturas/...; aquí solo se pide la ruta y se carga como cualquier imagen.
// Se cachean por la RUTA de la imagen original (que es el hash de su contenido), así que
// no hay nada que invalidar.
//
// Una ficha puede tener además una miniatura PROPIA (`Entidad.miniatura_url`): la que el usuario
// encuadró en su editor (ver `$lib/recorteFoto`), que el front recorta y Rust guarda tal cual con un
// delta. Si la tiene se usa esa directamente (su nombre lleva una versión, así que no hay caché que
// invalidar) y todo lo que dibuja la ficha (banco, «Fichas activas», arrastre, tablero) enseña la
// misma; si no, la automática de su foto.
//
// Memoria: lo que se guarda aquí es solo una ruta (unos bytes). Los píxeles los gestiona el
// webview, que descarta las imágenes de las filas que se desmontan al salir de pantalla
// (BancoEntidades solo pinta el <img> de las filas visibles). Por eso no hay límite ni LRU.
import { invoke } from "@tauri-apps/api/core";
import { alImportarImagen, urlDeImagen } from "$lib/imagenes";

type Tipo = "ficha" | "mapa";

/** Lo que hace falta de una ficha para saber qué miniatura tiene (sirve una `Entidad`). */
export interface FotoFicha {
  foto_url: string | null;
  miniatura_url: string | null;
}

class Miniaturas {
  // ruta original -> URL de la miniatura (o de la original si no se pudo reducir)
  #mapas = $state<Record<string, string>>({});
  #fichas = $state<Record<string, string>>({});
  // Peticiones en curso o ya resueltas (también las fallidas, para no repetirlas en bucle)
  #pedidas = new Set<string>();

  /** Miniatura de un mapa ("" mientras no esté lista). Solo lee; pedirla es `asegurar`. */
  de(ruta: string | null | undefined): string {
    return ruta ? (this.#mapas[ruta] ?? "") : "";
  }

  /** Miniatura de una ficha ("" mientras no esté lista o si no tiene foto). No lanza peticiones. */
  deFicha(ficha: FotoFicha): string {
    if (ficha.miniatura_url) return urlDeImagen(ficha.miniatura_url);
    return ficha.foto_url ? (this.#fichas[ficha.foto_url] ?? "") : "";
  }

  asegurar(ruta: string | null | undefined) {
    this.#pedir(ruta, "mapa");
  }

  /** Pide la automática de la foto, salvo que la ficha ya tenga la propia. */
  asegurarFicha(ficha: FotoFicha) {
    if (!ficha.miniatura_url) this.#pedir(ficha.foto_url, "ficha");
  }

  /** La imagen se acaba de (re)importar: su miniatura pudo haberse borrado, se vuelve a pedir. */
  olvidar(ruta: string) {
    this.#pedidas.delete(`mapa:${ruta}`);
    this.#pedidas.delete(`ficha:${ruta}`);
    delete this.#mapas[ruta];
    delete this.#fichas[ruta];
  }

  /** Se cambió de partida: las URLs guardadas apuntan a la carpeta de la anterior, se empieza de cero. */
  limpiar() {
    this.#pedidas.clear();
    this.#mapas = {};
    this.#fichas = {};
  }

  #pedir(ruta: string | null | undefined, tipo: Tipo) {
    if (!ruta) return;
    const clave = `${tipo}:${ruta}`;
    if (this.#pedidas.has(clave)) return;
    this.#pedidas.add(clave);

    const destino = tipo === "mapa" ? this.#mapas : this.#fichas;
    invoke<string | null>("obtener_miniatura", { ruta, tipo })
      .then((mini) => {
        // null = formato que no se puede reducir (svg, avif...) -> se usa la original
        destino[ruta] = urlDeImagen(mini ?? ruta);
      })
      .catch((err) => {
        console.warn("No se pudo obtener la miniatura de", ruta, err);
        destino[ruta] = urlDeImagen(ruta);
      });
  }
}

export const miniaturas = new Miniaturas();
alImportarImagen((ruta) => miniaturas.olvidar(ruta));
