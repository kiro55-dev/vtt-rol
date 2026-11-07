// src/lib/soltarArchivos.ts
//
// Archivos que se arrastran desde el sistema operativo y se sueltan sobre la ventana de la app.
// Tauri avisa a nivel de ventana (no de elemento), así que cada componente mira qué elemento hay
// debajo del cursor para decidir si el drop es suyo.
import { getCurrentWebview } from "@tauri-apps/api/webview";

type AlSoltar = (rutas: string[], elemento: Element | null) => void | Promise<unknown>;

/**
 * Llama a `alSoltar` con las rutas soltadas y el elemento bajo el cursor. Devuelve la función
 * que deja de escuchar, pensada para devolverla tal cual desde `onMount`.
 */
export function alSoltarArchivos(alSoltar: AlSoltar): () => void {
  let quitar: (() => void) | null = null;
  let terminado = false;

  try {
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type !== "drop") return;
        const rutas = event.payload.paths ?? [];
        if (rutas.length === 0) return;

        // Tauri da la posición en píxeles físicos; elementFromPoint los quiere en CSS
        const escala = window.devicePixelRatio || 1;
        const { x, y } = event.payload.position;
        const elemento = document.elementFromPoint(x / escala, y / escala);

        Promise.resolve(alSoltar(rutas, elemento)).catch((err) => console.error("Error al soltar archivos:", err));
      })
      .then((fn) => (terminado ? fn() : (quitar = fn)))
      .catch((err) => console.warn("dragDropEvent no disponible:", err));
  } catch (err) {
    console.warn("dragDropEvent no disponible:", err);
  }

  return () => {
    terminado = true;
    quitar?.();
  };
}
