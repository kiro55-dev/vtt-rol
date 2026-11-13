// src/lib/gestorPartidas.svelte.ts
//
// Lógica de la ventana de partidas (VentanaPartidas.svelte): abrir una, crear una nueva, y mostrar
// el estado ("guardando...") y el error. El trabajo de verdad lo hace `partida` (que guarda la
// rejilla pendiente, pide a Rust guardar la partida actual y abrir la otra, y recarga todo);
// aquí solo se evita que se lancen dos cambios a la vez y se recargan las tiradas.
import { partida } from "$lib/partida.svelte";
import { tiradas } from "$lib/tiradas.svelte";

export class GestorPartidas {
  /** La partida que se está renombrando (su fila es un campo de texto) y el texto escrito. */
  renombrando = $state<string | null>(null);
  textoNuevo = $state("");

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

  /**
   * Borra PARA SIEMPRE una partida (la ventana ya pidió confirmación). Vale también la abierta: Rust pasa antes a otra (o a
   * una nueva si era la única) y entonces se recargan las tiradas, que son de cada partida. Devuelve si se borró.
   */
  borrar(nombre: string): Promise<boolean> {
    return this.#hacer(() => partida.borrarPartida(nombre));
  }

  /** La fila de `nombre` pasa a ser un campo con su nombre actual. */
  empezarRenombrar(nombre: string) {
    if (this.ocupado) return;
    this.error = "";
    this.renombrando = nombre;
    this.textoNuevo = nombre;
  }

  cancelarRenombrar() {
    this.renombrando = null;
    this.error = "";
  }

  /** Cambia el nombre de una partida (vale también la abierta si la sala está cerrada). Devuelve si se cambió. */
  renombrar(nombre: string, nuevo: string): Promise<boolean> {
    return this.#hacer(() => partida.renombrarPartida(nombre, nuevo));
  }

  /** Guarda lo escrito en el campo. El mismo nombre no cambia nada; uno vacío avisa; los errores de Rust se ven. */
  async guardarRenombrado(): Promise<boolean> {
    const viejo = this.renombrando;
    if (viejo === null) return false;
    const nuevo = this.textoNuevo.trim();
    if (!nuevo) {
      this.error = "La partida necesita un nombre";
      return false;
    }
    if (nuevo === viejo) {
      this.renombrando = null;
      return true;
    }
    const hecho = await this.renombrar(viejo, nuevo);
    if (hecho) this.renombrando = null;
    return hecho;
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
