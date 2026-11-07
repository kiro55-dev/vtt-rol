// src/lib/gestorPartidas.svelte.ts
//
// Lógica de la ventana de partidas (VentanaPartidas.svelte): abrir una, crear una nueva, y mostrar
// el estado ("guardando...") y el error. El trabajo de verdad lo hace `partida` (que guarda la
// rejilla pendiente, pide a Rust guardar la partida actual y abrir la otra, y recarga todo);
// aquí solo se evita que se lancen dos cambios a la vez y se recargan las tiradas.
import { partida } from "$lib/partida.svelte";
import { tiradas } from "$lib/tiradas.svelte";

export class GestorPartidas {
  /** true mientras se guarda la partida actual y se abre la otra. */
  ocupado = $state(false);
  error = $state("");
  nombreNuevo = $state("");

  /** Lee la lista de partidas (al abrir la ventana). */
  async cargar() {
    try {
      await partida.recargarPartidas();
    } catch (err) {
      this.error = `No se pudo leer la lista de partidas: ${err}`;
    }
  }

  async #hacer(accion: () => Promise<void>): Promise<boolean> {
    if (this.ocupado) return false;
    this.ocupado = true;
    this.error = "";
    try {
      await accion();
      // El historial de tiradas y el de comandos (↑ ↓) son de cada partida
      await tiradas.cargar();
      return true;
    } catch (err) {
      this.error = err instanceof Error ? err.message : String(err);
      return false;
    } finally {
      this.ocupado = false;
    }
  }

  /** Abre otra partida. Devuelve false si no cambió (ya era la abierta, estaba ocupado o falló). */
  abrir(nombre: string): Promise<boolean> {
    if (nombre === partida.partidas.actual) return Promise.resolve(false);
    return this.#hacer(() => partida.cambiarPartida(nombre));
  }

  /** Crea la partida con el nombre escrito y pasa a ella. */
  async crear(): Promise<boolean> {
    const nombre = this.nombreNuevo.trim();
    if (!nombre) {
      this.error = "La partida necesita un nombre";
      return false;
    }
    const creada = await this.#hacer(() => partida.crearPartida(nombre));
    if (creada) this.nombreNuevo = "";
    return creada;
  }
}
