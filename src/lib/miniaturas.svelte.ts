// src/lib/miniaturas.svelte.ts
//
// Miniaturas de los mapas para el panel izquierdo. Se cachean por la RUTA de la imagen (que
// es el hash de su contenido): una imagen nueva siempre tiene miniatura nueva, dos mapas con
// la misma imagen la comparten, y nunca hay que "invalidar" nada.
import { urlDeImagen } from "$lib/imagenes";

// Configuración de resolución de fichas (fácil de ajustar)
export const RES_FICHA = { ancho: 48, alto: 48 };
const MAX_MINIATURAS_FICHA_CACHE = 15; // Límite para no saturar memoria RAM

async function generar(src: string, ancho: number, alto: number): Promise<string> {
  if (!src) return "";
  return new Promise((resolve) => {
    const img = new Image();
    img.crossOrigin = "anonymous";
    img.onload = () => {
      const canvas = document.createElement("canvas");
      canvas.width = ancho;
      canvas.height = alto;
      const ctx = canvas.getContext("2d");
      if (!ctx) return resolve("");
      ctx.drawImage(img, 0, 0, ancho, alto);
      resolve(canvas.toDataURL("image/webp", 0.9));
    };
    img.onerror = () => resolve("");
    img.src = src;
  });
}

class Miniaturas {
  // Mapas (Record reactivo)
  #mapas = $state<Record<string, string>>({});
  #pedidasMapas = new Set<string>();

  // Fichas (Record reactivo nativo en Svelte 5 para que la asignación despierte la interfaz)
  #fichas = $state<Record<string, string>>({});
  #pedidasFichas = new Set<string>();
  #ordenLRU: string[] = [];

  // --- Mapas ---
  de(ruta: string | null | undefined): string {
    return ruta ? (this.#mapas[ruta] ?? "") : "";
  }

  asegurar(ruta: string | null | undefined) {
    if (!ruta || this.#pedidasMapas.has(ruta)) return;
    this.#pedidasMapas.add(ruta);
    generar(urlDeImagen(ruta), 64, 36).then((mini) => {
      if (mini) this.#mapas[ruta] = mini;
    });
  }

  // --- Fichas ---
  deFicha(ruta: string | null | undefined): string {
    if (!ruta) return "";
    const mini = this.#fichas[ruta];
    if (mini) return mini;

    // Si aún no se ha pedido, la pide automáticamente
    this.asegurarFicha(ruta);
    return "";
  }

  asegurarFicha(ruta: string | null | undefined) {
    if (!ruta || this.#pedidasFichas.has(ruta) || this.#fichas[ruta]) return;
    this.#pedidasFichas.add(ruta);

    generar(urlDeImagen(ruta), RES_FICHA.ancho, RES_FICHA.alto).then((mini) => {
      this.#pedidasFichas.delete(ruta);
      if (!mini) return;

      // Gestión de memoria LRU
      this.#ordenLRU = this.#ordenLRU.filter((r) => r !== ruta);
      this.#ordenLRU.push(ruta);

      if (this.#ordenLRU.length > MAX_MINIATURAS_FICHA_CACHE) {
        const expirar = this.#ordenLRU.shift();
        if (expirar && expirar !== ruta) {
          delete this.#fichas[expirar];
        }
      }

      // Esta asignación directa activa inmediatamente la reactividad de Svelte 5
      this.#fichas[ruta] = mini;
    });
  }

  descargarFicha(ruta: string | null | undefined) {
    if (!ruta) return;
    delete this.#fichas[ruta];
    this.#pedidasFichas.delete(ruta);
    this.#ordenLRU = this.#ordenLRU.filter((r) => r !== ruta);
  }
}

export const miniaturas = new Miniaturas();