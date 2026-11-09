// src/lib/tiradas.svelte.ts
//
// Estado compartido de las tiradas de dados: el historial que se muestra, el texto del chat y
// todo lo que se hace con él (añadir/quitar dados desde la barra de dados, tirar, navegar por los
// comandos anteriores). La barra de dados, la lista de tiradas y la caja de texto lo comparten
// sin pasarse nada entre ellas.
import { invoke } from "$lib/backend";
import { esJugador } from "$lib/modo";

export interface Tirada {
  hora: string;
  /** Desglose ya formateado por Rust (puede llevar <b>negritas</b>), o un mensaje de error. */
  comando: string;
  /** `null` = la tirada no era válida (no hay total). */
  total: number | null;
  /** Quién tiró (su nombre), para el chat compartido: «Ana: 1d20 (7) = 7». Vacío si fue el host. */
  autor?: string;
  /** `false`: solo la ve el host (una tirada privada). */
  publica?: boolean;
}

const MAX_TIRADAS = 20;

const horaActual = () => new Date().toTimeString().slice(0, 5);

/**
 * Añade (`esSuma`) o quita un dado al comando del chat, agrupando los iguales:
 * "1d20" + "1d6" -> "1d20 + 1d6"; otra vez "1d6" -> "1d20 + 2d6"; restando se va reduciendo
 * hasta desaparecer. Función pura (no toca el estado) para poder probarla.
 */
export function aplicarDadoAlComando(comando: string, fragmento: string, esSuma: boolean): string {
  const matchFrag = fragmento.match(/^(\d*)d(\d+)([!a-z]*)$/i);
  if (!matchFrag) return comando;

  const delta = (matchFrag[1] ? parseInt(matchFrag[1]) : 1) * (esSuma ? 1 : -1);
  const caras = matchFrag[2];
  const sufijos = matchFrag[3] || "";

  const sufijosEscapados = sufijos.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const regex = new RegExp(`(^|[+-])\\s*(\\d*)d${caras}${sufijosEscapados}(?=\\s|[+-]|$)`, "i");
  const match = comando.match(regex);

  if (!match) {
    if (esSuma) {
      const texto = `${matchFrag[1] || 1}d${caras}${sufijos}`;
      return comando.trim() ? `${comando.trim()} + ${texto}` : texto;
    }
    const texto = `- ${matchFrag[1] || 1}d${caras}${sufijos}`;
    return comando.trim() ? `${comando.trim()} ${texto}` : texto;
  }

  const signoActual = match[1] === "-" ? -1 : 1;
  const vecesActuales = (match[2] ? parseInt(match[2]) : 1) * signoActual;
  const nuevaCantidad = vecesActuales + delta;

  let resultado: string;
  if (nuevaCantidad === 0) {
    resultado = comando.replace(regex, "");
  } else if (nuevaCantidad > 0) {
    const prefijo = match.index === 0 ? "" : "+ ";
    resultado = comando.replace(regex, `${prefijo}${nuevaCantidad}d${caras}${sufijos}`);
  } else {
    resultado = comando.replace(regex, `- ${Math.abs(nuevaCantidad)}d${caras}${sufijos}`);
  }

  return resultado
    .replace(/^\s*\+\s*/, "")
    .replace(/\s*[+-]\s*$/, "")
    .replace(/\s*\+\s*\+/g, " + ")
    .replace(/\s*\+\s*-\s*/g, " - ")
    .trim();
}

/**
 * El desglose listo para `{@html}`: escapa TODO y solo vuelve a permitir `<b>` y `</b>` (las negritas que
 * pone Rust en el 1 y en la cara máxima). Rust ya solo genera números y palabras fijas, así que es una
 * defensa de más: aunque algún día llegara por red un desglose manipulado, no se ejecutaría nada.
 */
export function desgloseSeguro(desglose: string): string {
  return desglose
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;")
    .replace(/&lt;(\/?)b&gt;/g, "<$1b>");
}

/**
 * `clean` o `c` (solos, da igual mayúsculas) vacían el chat. Mezclados con otra cosas (`c + 1d6`) no
 * lo son: van a Rust como tirada y no es válida. Igual que `es_comando_limpiar` de Rust.
 */
export function esComandoLimpiar(comando: string): boolean {
  return /^(clean|c)$/i.test(comando.trim());
}

class Tiradas {
  /** Las últimas tiradas, la más reciente primero. */
  historial = $state<Tirada[]>([]);
  /** Lo que hay escrito en la caja de comandos. */
  comando = $state("");
  /** Solo el host: si las tiradas que hace las ven los jugadores (casilla de la barra de dados). */
  publica = $state(true);

  // Navegación por los comandos anteriores con ↑ ↓, como en una terminal.
  #anteriores: string[] = [];
  #indice = -1; // -1 = no se está navegando
  #borrador = ""; // lo que se estaba escribiendo antes de empezar a navegar

  #enfocar: (() => void) | null = null;

  /** Carga el historial guardado en Rust. */
  async cargar() {
    // El chat es de todos: cada tirada lleva el nombre de quien la hizo (`autor`, vacío si fue el host)
    const guardadas = await invoke<{ hora: string; comando: string; total: number; autor?: string; publica?: boolean }[]>("obtener_historial_tiradas");
    this.historial = guardadas.map((t) => ({ hora: t.hora, comando: t.comando, total: t.total, autor: t.autor ?? "", publica: t.publica ?? true }));
    this.#salirDelHistorial(); // los comandos anteriores (↑ ↓) también son de la partida abierta
  }

  // ------------------------------------------------------------------
  // Composición del comando (barra de dados)
  // ------------------------------------------------------------------

  sumarDado(fragmento: string) {
    this.comando = aplicarDadoAlComando(this.comando, fragmento, true);
    this.#enfocar?.();
  }

  restarDado(fragmento: string) {
    this.comando = aplicarDadoAlComando(this.comando, fragmento, false);
    this.#enfocar?.();
  }

  /** La caja de texto se registra aquí para que otros puedan pedirle el foco. Devuelve cómo darse de baja. */
  registrarFoco(enfocar: () => void): () => void {
    this.#enfocar = enfocar;
    return () => {
      if (this.#enfocar === enfocar) this.#enfocar = null;
    };
  }

  // ------------------------------------------------------------------
  // Tirar
  // ------------------------------------------------------------------

  /** Intro en la caja de texto: tira lo escrito y la vacía enseguida. */
  async enviar() {
    const cmd = this.comando.trim();
    if (!cmd) return;
    this.comando = "";
    this.#salirDelHistorial();
    await this.tirar(cmd);
  }

  /** Botón TIRAR de la barra de dados: lo escrito más el bonificador. */
  async tirarConBonificador(bonificador: number | null) {
    if (!this.comando.trim()) return;
    const bonif = Number(bonificador) || 0; // un campo vacío (null) cuenta como 0
    let final = this.comando;
    if (bonif !== 0) final += bonif > 0 ? ` + ${bonif}` : ` - ${Math.abs(bonif)}`;
    this.comando = "";
    this.#salirDelHistorial();
    await this.tirar(final);
  }

  async tirar(comando: string) {
    const cmd = comando.trim();
    if (!cmd) return;

    if (esComandoLimpiar(cmd)) {
      try {
        await invoke("limpiar_chat", { comando: cmd });
        this.historial = [];
      } catch (err) {
        console.error("limpiar_chat:", err);
        // Solo el host limpia el chat de todos
        if (esJugador) this.#anadir({ hora: horaActual(), comando: "Solo el host puede limpiar el chat", total: null });
      }
      return;
    }

    try {
      // Solo el host puede tirar en privado (las de los jugadores son siempre públicas)
      await invoke<string>("tirar_dado", esJugador ? { comando: cmd } : { comando: cmd, publica: this.publica });
      // La tirada ya está en el chat de la partida (con el nombre de quien tira): se vuelve a leer, así sale
      // igual para todos y no se duplica cuando llega también como cambio
      await this.cargar();
    } catch {
      this.#anadir({ hora: horaActual(), comando: "tirada no válida", total: null });
    }
  }

  #anadir(tirada: Tirada) {
    this.historial.unshift(tirada);
    if (this.historial.length > MAX_TIRADAS) this.historial.pop();
  }

  // ------------------------------------------------------------------
  // Historial de comandos (↑ ↓)
  // ------------------------------------------------------------------

  get navegandoHistorial() {
    return this.#indice !== -1;
  }

  /** ↑: pone en la caja el comando anterior. Devuelve `true` si cambió el texto. */
  async historialAnterior(): Promise<boolean> {
    if (this.#indice === -1) {
      this.#anteriores = await invoke<string[]>("obtener_historial_personal");
      if (this.#anteriores.length === 0) return false;
      this.#borrador = this.comando; // se guarda lo que estaba escribiendo
      this.#indice = this.#anteriores.length;
    }
    if (this.#indice <= 0) return false;

    this.#indice--;
    this.comando = this.#anteriores[this.#indice];
    return true;
  }

  /** ↓: avanza hacia los más recientes; al pasar el último, devuelve lo que se estaba escribiendo. */
  historialSiguiente(): boolean {
    if (this.#indice === -1) return false;
    if (this.#indice >= this.#anteriores.length) return false;

    this.#indice++;
    if (this.#indice === this.#anteriores.length) {
      this.comando = this.#borrador;
      this.#indice = -1;
    } else {
      this.comando = this.#anteriores[this.#indice];
    }
    return true;
  }

  #salirDelHistorial() {
    this.#indice = -1;
    this.#borrador = "";
  }
}

export const tiradas = new Tiradas();
