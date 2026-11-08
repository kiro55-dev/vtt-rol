// src/lib/visorImagen.svelte.ts
//
// La foto de una ficha a resolución completa (doble clic en una tarjeta de «Fichas activas»). Aquí solo
// está la RUTA de la imagen abierta y dónde se abrió; el componente `VisorImagen` la carga del archivo
// original y, al cerrar, la suelta (así los originales, que pesan mucho más que las miniaturas, no se
// acumulan).
class VisorImagen {
  /** Ruta relativa de la imagen abierta (`imagenes/...`), o null si no hay ninguna. */
  ruta = $state<string | null>(null);
  /** Posición del puntero al abrirla (px de la ventana): ahí queda la esquina superior derecha. */
  x = $state(0);
  y = $state(0);

  abrir(ruta: string | null | undefined, x = 0, y = 0) {
    if (!ruta) return;
    this.ruta = ruta;
    this.x = x;
    this.y = y;
  }

  cerrar() {
    this.ruta = null;
  }
}

export const visorImagen = new VisorImagen();
