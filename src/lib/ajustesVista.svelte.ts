// src/lib/ajustesVista.svelte.ts
//
// Preferencias de cómo se ve la pantalla, solo de este equipo: no son parte de la partida, así que ni
// pasan por Rust ni son deltas. Se recuerdan en localStorage (si no se puede, se usan las de serie).

const CLAVE = "openroll.ajustesVista";

interface Guardado {
  edicionRejilla?: boolean;
}

function leer(): Guardado {
  try {
    return JSON.parse(localStorage.getItem(CLAVE) ?? "{}") ?? {};
  } catch {
    return {};
  }
}

class AjustesVista {
  /** Si se ve la barra para editar la rejilla (arriba del tablero). La rejilla se dibuja igual. */
  edicionRejilla = $state(leer().edicionRejilla ?? true);

  alternarEdicionRejilla() {
    this.edicionRejilla = !this.edicionRejilla;
    this.#guardar();
  }

  #guardar() {
    try {
      localStorage.setItem(CLAVE, JSON.stringify({ edicionRejilla: this.edicionRejilla } satisfies Guardado));
    } catch {
      // Sin almacenamiento (ventana privada, tests...): solo dura esta sesión
    }
  }
}

export const ajustesVista = new AjustesVista();
