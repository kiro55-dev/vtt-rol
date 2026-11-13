// src/lib/arrastreAjeno.svelte.ts
//
// Las fichas que OTROS están arrastrando ahora mismo por el tablero. Llegan como avisos efímeros
// (`Arrastre` y `ArrastreFin`, ver avisosTablero.ts): no cambian la partida, solo dejan ver por qué
// casillas pasa la ficha. La posición real de la instancia sigue donde estaba hasta que el otro la suelta
// (entonces llega el cambio de verdad).
import type { Casilla } from "$lib/partida.svelte";
import type { AvisoTablero } from "$lib/avisosTablero";

/** Si el emisor deja de dar señales (se desconecta a mitad), la ficha vuelve sola a su sitio. */
export const CADUCA_SIN_LATIDO_MS = 10_000;
/** Al soltar se mantiene un instante, hasta que llega la posición nueva, para que no «salte atrás». */
export const PERMANENCIA_TRAS_SOLTAR_MS = 1_500;

/** Un `Arrastre` que llega justo después de soltar y con número menor es uno viejo que llegó tarde (canal sin orden). */
export const VENTANA_RETRASADOS_MS = 300;

export interface PiezaAjena {
  idMapa: number;
  casilla: Casilla;
  caduca: number;
  soltada: boolean;
  /** Número del último aviso que movió esta ficha (0: de quien aún no los numera). */
  n: number;
  /** Cuándo se soltó (para reconocer un `Arrastre` retrasado). */
  finEn: number;
}

class ArrastreAjeno {
  /** Por id de instancia. Se sustituye entero al cambiar (así lo ve quien lo lee en un efecto). */
  piezas = $state.raw<Record<number, PiezaAjena>>({});
  #temporizador: ReturnType<typeof setTimeout> | null = null;

  /** Atiende un aviso del tablero; los que no son de arrastre se ignoran. */
  recibir(aviso: AvisoTablero, ahora = Date.now()) {
    if ("Arrastre" in aviso) {
      const { id_mapa, piezas, n = 0 } = aviso.Arrastre;
      const nuevas = { ...this.piezas };
      for (const p of piezas) {
        const antes = nuevas[p.id_instancia];
        // Por el canal sin orden puede llegar uno viejo: mientras se arrastra, o justo tras soltar, se ignora el de
        // número menor. (Pasado un rato, otro que arrastre la misma ficha empieza su cuenta de nuevo y vale.)
        if (antes && n > 0 && antes.n > 0) {
          const viejo = antes.soltada ? ahora - antes.finEn < VENTANA_RETRASADOS_MS && n <= antes.n : n < antes.n;
          if (viejo) continue;
        }
        nuevas[p.id_instancia] = { idMapa: id_mapa, casilla: p.casilla, caduca: ahora + CADUCA_SIN_LATIDO_MS, soltada: false, n, finEn: 0 };
      }
      this.#poner(nuevas, ahora);
    } else if ("ArrastreFin" in aviso) {
      const nuevas = { ...this.piezas };
      const n = aviso.ArrastreFin.n ?? 0;
      for (const id of aviso.ArrastreFin.ids_instancias) {
        const p = nuevas[id];
        if (!p || (n > 0 && p.n > n)) continue; // un fin más viejo que lo último visto no la suelta
        nuevas[id] = { ...p, caduca: ahora + PERMANENCIA_TRAS_SOLTAR_MS, soltada: true, n: Math.max(p.n, n), finEn: ahora };
      }
      this.#poner(nuevas, ahora);
    }
  }

  /** La casilla por la que pasa ahora esa instancia en ese mapa (null si nadie la arrastra). */
  de(idInstancia: number, idMapa: number | null): Casilla | null {
    const p = this.piezas[idInstancia];
    return p && p.idMapa === idMapa ? p.casilla : null;
  }

  /**
   * Quita las ya soltadas cuyo cambio real ha llegado: `posiciones` son las casillas que tienen ahora las
   * instancias del mapa abierto (id → casilla).
   */
  alLlegarPosiciones(posiciones: ReadonlyMap<number, Casilla>) {
    const nuevas = { ...this.piezas };
    let cambia = false;
    for (const [id, p] of Object.entries(nuevas)) {
      const real = posiciones.get(Number(id));
      if (p.soltada && real && real.x === p.casilla.x && real.y === p.casilla.y) {
        delete nuevas[Number(id)];
        cambia = true;
      }
    }
    if (cambia) this.#poner(nuevas, Date.now());
  }

  /** Olvida todo (otro mapa, otra partida). */
  limpiar() {
    if (Object.keys(this.piezas).length === 0) return;
    if (this.#temporizador) clearTimeout(this.#temporizador);
    this.#temporizador = null;
    this.piezas = {};
  }

  #poner(nuevas: Record<number, PiezaAjena>, ahora: number) {
    this.piezas = nuevas;
    this.#programarPoda(ahora);
  }

  // Quita las caducadas, en el momento en que caduca la primera
  #programarPoda(ahora: number) {
    if (this.#temporizador) clearTimeout(this.#temporizador);
    this.#temporizador = null;
    const caducas = Object.values(this.piezas).map((p) => p.caduca);
    if (caducas.length === 0) return;
    this.#temporizador = setTimeout(() => {
      const t = Date.now();
      const vivas = Object.fromEntries(Object.entries(this.piezas).filter(([, p]) => p.caduca > t));
      this.#poner(vivas as Record<number, PiezaAjena>, t);
    }, Math.max(0, Math.min(...caducas) - ahora) + 5);
  }
}

export const arrastreAjeno = new ArrastreAjeno();
