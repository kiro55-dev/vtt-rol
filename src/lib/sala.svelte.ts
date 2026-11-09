// src/lib/sala.svelte.ts
//
// La sala de la partida abierta, para la ventana «Configuración de sala» (VentanaSala.svelte). Todo vive en
// Rust (src-tauri/src/red/): aquí solo se llama a sus comandos y se vuelve a leer cuando Rust avisa con el
// evento `sala-cambiada` (al abrirse, durante la descarga de cloudflared, cuando alguien entra o sale...).
import { invoke } from "$lib/backend";
import { listen } from "$lib/backend";
import { partida } from "$lib/partida.svelte";

export type Fase =
  | { tipo: "Cerrada" }
  | { tipo: "Preparando"; paso: string; progreso: number | null }
  | { tipo: "Abierta" };

/** Espejo de `red::EstadoSala`. */
export interface EstadoSala {
  fase: Fase;
  enlace: string | null;
  pide_contrasena: boolean;
  conectados: number[];
  /** Los mapas que ven los jugadores (los de la casilla marcada, de todas las zonas). */
  mapas_compartidos: { id: number; nombre: string }[];
  error: string | null;
}

/** Espejo de `red::comandos::JugadorSala`. */
export interface JugadorSala {
  id: number;
  nombre: string;
  conectado: boolean;
  /** Ids de las fichas del banco de las que es propietario. */
  fichas: number[];
}

const CERRADA: EstadoSala = { fase: { tipo: "Cerrada" }, enlace: null, pide_contrasena: false, conectados: [], mapas_compartidos: [], error: null };

class Sala {
  estado = $state<EstadoSala>({ ...CERRADA });
  jugadores = $state<JugadorSala[]>([]);
  /** Si la ventana está abierta. */
  ventanaAbierta = $state(false);

  abierta = $derived(this.estado.fase.tipo === "Abierta");
  /** Abierta o abriéndose: en los dos casos no se puede cambiar de partida. */
  enMarcha = $derived(this.estado.fase.tipo !== "Cerrada");

  #escuchando = false;

  /** Empieza a escuchar a Rust y lee el estado (una vez; la llama +page al montar). */
  async iniciar() {
    if (!this.#escuchando) {
      this.#escuchando = true;
      await listen("sala-cambiada", () => void this.recargar()).catch((e) => console.warn("sala-cambiada:", e));
    }
    await this.recargar();
  }

  async recargar() {
    const [estado, jugadores] = await Promise.all([
      invoke<EstadoSala>("estado_sala"),
      invoke<JugadorSala[]>("obtener_jugadores")
    ]);
    this.estado = estado;
    this.jugadores = jugadores;
  }

  // Los errores se quedan en `estado.error` (los pone Rust) o se propagan para que los enseñe la ventana

  async abrir() {
    try {
      await invoke("abrir_sala");
    } finally {
      await this.recargar();
    }
  }

  async cerrar() {
    await invoke("cerrar_sala");
    await this.recargar();
  }

  async regenerarEnlace() {
    await invoke("regenerar_enlace");
    await this.recargar();
  }

  /** `null` o "" = sin contraseña. */
  async ponerContrasena(contrasena: string | null) {
    await invoke("poner_contrasena", { contrasena: contrasena || null });
    await this.recargar();
  }

  async expulsar(id: number) {
    await invoke("expulsar_jugador", { id });
    await this.recargar();
  }

  /** El jugador pasa a controlar la ficha (soltarlo encima, ver arrastreJugador). */
  async asignar(idJugador: number, idEntidad: number) {
    await invoke("asignar_propietario", { idEntidad, idJugador });
    await Promise.all([this.recargar(), partida.recargarEntidades()]);
  }

  async quitar(idJugador: number, idEntidad: number) {
    await invoke("quitar_propietario", { idEntidad, idJugador });
    await Promise.all([this.recargar(), partida.recargarEntidades()]);
  }

  async copiarEnlace(): Promise<boolean> {
    if (!this.estado.enlace) return false;
    try {
      await navigator.clipboard.writeText(this.estado.enlace);
      return true;
    } catch {
      return false;
    }
  }
}

export const sala = new Sala();
