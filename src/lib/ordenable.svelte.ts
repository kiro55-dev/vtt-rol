// src/lib/ordenable.svelte.ts
//
// Lista que se reordena arrastrando con el puntero, con la previsualización en vivo y el
// clon flotante que usan las zonas y los mapas. Solo escribe en Rust al soltar.
type ConId = { id: number; orden?: number };

interface Opciones<T> {
  /** Selector CSS de cada tarjeta (para saber sobre cuál está el cursor). Deben llevar data-indice. */
  selector: string;
  /** Recibe los ids en el orden nuevo. Debe terminar de recargar la lista. */
  alReordenar: (ids: number[]) => Promise<void> | void;
  /** Se llama cuando fue un clic y no un arrastre. */
  alPulsar: (item: T) => void;
  /** Para impedir arrastrar mientras se edita un nombre. */
  bloqueado?: () => boolean;
  /** Sin reordenar: pulsar solo elige (los jugadores no cambian el orden de la partida). */
  soloClic?: boolean;
}

export class ListaOrdenable<T extends ConId> {
  /** La lista tal como se ve (se reordena en vivo mientras se arrastra). */
  visuales = $state<T[]>([]);

  activo = $state(false);
  item = $state<T | null>(null);
  x = $state(0);
  y = $state(0);
  ancho = $state(0);
  alto = $state(0);
  offsetX = $state(0);
  offsetY = $state(0);

  #opciones: Opciones<T>;

  constructor(opciones: Opciones<T>) {
    this.#opciones = opciones;
  }

  /** Copia (ordenada) los datos reales a la lista visual, salvo en pleno arrastre. */
  sincronizar(items: readonly T[]) {
    if (this.activo) return;
    this.visuales = [...items].sort((a, b) => (a.orden ?? 0) - (b.orden ?? 0));
  }

  iniciar(item: T, indice: number, e: PointerEvent) {
    if (e.button !== 0) return; // solo botón principal
    if (this.#opciones.bloqueado?.()) return;
    if (this.#opciones.soloClic) {
      const alSoltarClic = () => {
        window.removeEventListener("pointerup", alSoltarClic);
        this.#opciones.alPulsar(item);
      };
      window.addEventListener("pointerup", alSoltarClic);
      return;
    }

    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const inicioX = e.clientX;
    const inicioY = e.clientY;
    let superadoUmbral = false;
    let indiceDestino = indice;

    const alMover = (ev: PointerEvent) => {
      if (!superadoUmbral && Math.hypot(ev.clientX - inicioX, ev.clientY - inicioY) > 4) {
        superadoUmbral = true;
        this.activo = true;
        this.item = item;
        this.ancho = rect.width;
        this.alto = rect.height;
        this.offsetX = inicioX - rect.left;
        this.offsetY = inicioY - rect.top;
      }
      if (!this.activo) return;

      this.x = ev.clientX;
      this.y = ev.clientY;

      // ¿Sobre qué tarjeta está el cursor?
      const sobre = document.elementFromPoint(ev.clientX, ev.clientY)?.closest<HTMLElement>(this.#opciones.selector);
      if (sobre?.dataset.indice === undefined) return;

      const nuevo = Number(sobre.dataset.indice);
      if (nuevo === indiceDestino) return;
      indiceDestino = nuevo;

      const copia = [...this.visuales];
      const [movido] = copia.splice(copia.findIndex((v) => v.id === item.id), 1);
      copia.splice(nuevo, 0, movido);
      this.visuales = copia;
    };

    const alSoltar = async () => {
      window.removeEventListener("pointermove", alMover);
      window.removeEventListener("pointerup", alSoltar);

      if (!superadoUmbral) {
        this.#opciones.alPulsar(item);
        return;
      }

      try {
        await this.#opciones.alReordenar(this.visuales.map((v) => v.id));
      } catch (err) {
        console.error("No se pudo guardar el nuevo orden:", err);
      } finally {
        this.activo = false;
        this.item = null;
      }
    };

    window.addEventListener("pointermove", alMover);
    window.addEventListener("pointerup", alSoltar);
  }
}
