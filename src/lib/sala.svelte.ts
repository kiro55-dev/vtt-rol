// src/lib/sala.svelte.ts
//
// La sala de la partida abierta, para la ventana «Configuración de sala» (VentanaSala.svelte). Todo vive en
// Rust (src-tauri/src/red/): aquí solo se llama a sus comandos y se vuelve a leer cuando Rust avisa con el
// evento `sala-cambiada` (al abrirse, durante la descarga de cloudflared, cuando alguien entra o sale...).
import { invoke } from "$lib/backend";
import { listen } from "$lib/backend";
import { partida } from "$lib/partida.svelte";
import { notificaciones } from "$lib/notificaciones.svelte";

export type Fase =
  | { tipo: "Cerrada" }
  | { tipo: "Preparando"; paso: string; progreso: number | null }
  | { tipo: "Abierta" };

/** Espejo de `red::config::Respaldo`: cómo se publica la sala en internet. */
export type Respaldo = { tipo: "Cloudflare" } | { tipo: "Propio"; url: string; puerto: number } | { tipo: "Lan"; puerto: number };

/** Espejo de `red::EnlaceSala`: un enlace para los jugadores y de qué red es (en red local, una por IP). */
export interface EnlaceSala {
  etiqueta: string;
  url: string;
}

/** Espejo de `red::config::ConfigRed` (`<datos de la app>/red.json`, de este equipo). */
export interface ConfigRed {
  respaldo: Respaldo;
  /** No ofrecer la conexión directa: los jugadores no verán la IP del host. */
  solo_respaldo: boolean;
}

/** Espejo de `red::EstadoSala`. */
export interface EstadoSala {
  fase: Fase;
  enlace: string | null;
  /** «Solo respaldo»: el de la sala abierta o, cerrada, el que se usará al abrirse. */
  solo_respaldo: boolean;
  pide_contrasena: boolean;
  conectados: number[];
  /** Los mapas que ven los jugadores (los de la casilla marcada, de todas las zonas). */
  mapas_compartidos: { id: number; nombre: string }[];
  error: string | null;
  /** El que usa la sala abierta o, cerrada, el que usará al abrirse. */
  respaldo: Respaldo;
  /** Todos los enlaces (en red local, uno por cada IP del equipo); `enlace` es el primero. */
  enlaces: EnlaceSala[];
  /** Si la app lleva dentro la página de los jugadores (hace falta para la dirección propia y la red local). */
  pagina_incluida: boolean;
  /** Cuántos relés de Nostr tiene conectados la escucha de ofertas (solo en peer to peer, con la sala abierta). */
  senal: { reles_conectados: number; reles_total: number } | null;
}

/** Por dónde entró un jugador: directo (peer to peer) o por el respaldo (túnel, dirección propia o red local). */
export type ViaJugador = "directo" | "respaldo";

/** Espejo de `red::comandos::JugadorSala`. */
export interface JugadorSala {
  id: number;
  nombre: string;
  conectado: boolean;
  /** Por dónde entró, si está conectado. */
  via: ViaJugador | null;
  /** Ids de las fichas del banco de las que es propietario. */
  fichas: number[];
}

const CERRADA: EstadoSala = {
  fase: { tipo: "Cerrada" },
  enlace: null,
  pide_contrasena: false,
  conectados: [],
  mapas_compartidos: [],
  error: null,
  respaldo: { tipo: "Cloudflare" },
  enlaces: [],
  pagina_incluida: true,
  senal: null,
  solo_respaldo: false
};

/** Lo que dice la comprobación de versión (ver `red/version.rs`): «fuerte» = los jugadores no podrán entrar. */
export interface AvisoVersion {
  nivel: "fuerte" | "suave";
  texto: string;
}

class Sala {
  /** Si la web de los jugadores es más nueva que esta app (solo con el túnel de Cloudflare): se mira al abrir la sala. */
  avisoVersion = $state<AvisoVersion | null>(null);
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
    void this.#comprobarVersion(); // sala abierta: ¿podrán entrar los jugadores con la versión de esta app?
  }

  /** Pregunta a Rust si la web de los jugadores es más nueva que la app. Sin red o sin Cloudflare, no hay aviso. */
  async #comprobarVersion() {
    try {
      const aviso = await invoke<AvisoVersion | null>("comprobar_version_web");
      this.avisoVersion = aviso ?? null;
      if (aviso) notificaciones.mostrar(aviso.texto, aviso.nivel === "fuerte" ? 15000 : 8000, { largo: true, aviso: aviso.nivel === "fuerte" });
    } catch {
      this.avisoVersion = null;
    }
  }

  async cerrar() {
    this.avisoVersion = null;
    await invoke("cerrar_sala");
    await this.recargar();
  }

  /** Cómo se publica la sala en este equipo. */
  leerConfig(): Promise<ConfigRed> {
    return invoke<ConfigRed>("obtener_config_red");
  }

  /** Solo con la sala cerrada; Rust comprueba la dirección y el puerto (el error llega tal cual). */
  async guardarConfig(config: ConfigRed) {
    await invoke("guardar_config_red", { config });
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

  /** Lo quita de la partida (y de la sala, si está): sus fichas se quedan, pero sin dueño. */
  async eliminar(id: number) {
    await invoke("eliminar_jugador", { id });
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

  async copiarEnlace(url = this.estado.enlace): Promise<boolean> {
    if (!url) return false;
    try {
      await navigator.clipboard.writeText(url);
      return true;
    } catch {
      return false;
    }
  }
}

export const sala = new Sala();
