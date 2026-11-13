// src/lib/marcasTablero.ts
// Marcas pasajeras que se dibujan encima del tablero: el "ping" (clic central) y la flecha de un
// recorrido (arrastre con el botón derecho). No son estado de la partida, solo dibujo (ver
// avisosTablero.ts: se avisan a Rust como avisos efímeros, y los de otros jugadores se pintan con
// estas mismas funciones).
import type { Casilla } from "$lib/partida.svelte";
import { centroDe, mundoAPantalla, type Camara, type RejillaTablero } from "$lib/tablero";
import { COLOR_VERDE, rgbCss, type ColorMarca } from "$lib/colorMarca";

// ---------------------------------------------------------------------------
// Ping
// ---------------------------------------------------------------------------

/** Duración total de un ping (ms). */
export const DURACION_PING_MS = 1000;
/** El amarillo del ping de antes de que cada uno tuviera su color. */
const COLOR_PING: ColorMarca = [250, 204, 21];
// Dos ondas que crecen desde el centro de la casilla; la segunda sale un poco después.
const ONDAS = [0, 0.25];
const DURACION_ONDA = 0.75; // fracción del total
// Radio final en casillas: 1.5 cubre la casilla pulsada y las de alrededor (3×3).
const RADIO_CASILLAS = 1.5;

export interface Ping {
  casilla: Casilla;
  inicio: number; // performance.now()
  /** El color de quien lo hizo (si no, el amarillo de siempre). */
  color?: ColorMarca;
}

/** Radio (0..1 del máximo) y opacidad de una onda; null si aún no ha salido o ya terminó. */
export function onda(progreso: number, retraso: number): { radio: number; alfa: number } | null {
  const t = (progreso - retraso) / DURACION_ONDA;
  if (t < 0 || t > 1) return null;
  const radio = 1 - (1 - t) ** 3; // sale rápido y frena al final
  return { radio, alfa: 1 - t };
}

export function pingVivo(ping: Ping, ahora: number): boolean {
  return ahora - ping.inicio < DURACION_PING_MS;
}

export function dibujarPing(ctx: CanvasRenderingContext2D, cam: Camara, rejilla: RejillaTablero, ping: Ping, ahora: number) {
  const progreso = (ahora - ping.inicio) / DURACION_PING_MS;
  const centro = centroDe(rejilla, ping.casilla);
  const p = mundoAPantalla(cam, centro.x, centro.y);
  const radioMax = RADIO_CASILLAS * Math.max(rejilla.anchoCasilla, rejilla.altoCasilla) * cam.zoom;
  const grosor = Math.max(2, Math.min(6, radioMax * 0.06));
  const color = ping.color ?? COLOR_PING;

  ctx.save();
  for (const retraso of ONDAS) {
    const o = onda(progreso, retraso);
    if (!o) continue;
    ctx.globalAlpha = o.alfa;
    ctx.beginPath();
    ctx.arc(p.x, p.y, Math.max(1, o.radio * radioMax), 0, Math.PI * 2);
    ctx.fillStyle = rgbCss(color, 0.15);
    ctx.fill();
    ctx.lineWidth = grosor;
    ctx.strokeStyle = rgbCss(color);
    ctx.stroke();
  }
  // Punto central que se apaga durante todo el ping
  ctx.globalAlpha = Math.max(0, 1 - progreso);
  ctx.beginPath();
  ctx.arc(p.x, p.y, grosor * 1.5, 0, Math.PI * 2);
  ctx.fillStyle = rgbCss(color);
  ctx.fill();
  ctx.restore();
}

// ---------------------------------------------------------------------------
// Recorrido
// ---------------------------------------------------------------------------

/**
 * Flecha que une los centros de las casillas del recorrido, con la casilla de salida marcada y el
 * número de casillas recorridas junto a la punta.
 */
export function dibujarRecorrido(
  ctx: CanvasRenderingContext2D,
  cam: Camara,
  rejilla: RejillaTablero,
  recorrido: readonly Casilla[],
  color: ColorMarca = COLOR_VERDE
) {
  if (recorrido.length === 0) return;
  const casillaPx = Math.min(rejilla.anchoCasilla, rejilla.altoCasilla) * cam.zoom;
  const puntos = recorrido.map((c) => {
    const m = centroDe(rejilla, c);
    return mundoAPantalla(cam, m.x, m.y);
  });

  ctx.save();

  // Casilla de salida
  const salida = recorrido[0];
  const esquina = mundoAPantalla(cam, salida.x * rejilla.anchoCasilla, salida.y * rejilla.altoCasilla);
  ctx.fillStyle = rgbCss(color, 0.4);
  ctx.fillRect(esquina.x, esquina.y, rejilla.anchoCasilla * cam.zoom, rejilla.altoCasilla * cam.zoom);

  if (puntos.length > 1) {
    const grosor = Math.max(2, Math.min(8, casillaPx * 0.12));
    const punta = Math.max(8, grosor * 3);
    const fin = puntos[puntos.length - 1];
    const previo = puntos[puntos.length - 2];
    const ang = Math.atan2(fin.y - previo.y, fin.x - previo.x);
    // La línea acaba donde empieza la punta, para que no asome por delante
    const finLinea = { x: fin.x - Math.cos(ang) * punta * 0.8, y: fin.y - Math.sin(ang) * punta * 0.8 };

    const trazar = () => {
      ctx.beginPath();
      ctx.moveTo(puntos[0].x, puntos[0].y);
      for (const p of puntos.slice(1, -1)) ctx.lineTo(p.x, p.y);
      ctx.lineTo(finLinea.x, finLinea.y);
    };
    const triangulo = () => {
      ctx.beginPath();
      ctx.moveTo(fin.x, fin.y);
      ctx.lineTo(fin.x - Math.cos(ang - 0.45) * punta, fin.y - Math.sin(ang - 0.45) * punta);
      ctx.lineTo(fin.x - Math.cos(ang + 0.45) * punta, fin.y - Math.sin(ang + 0.45) * punta);
      ctx.closePath();
    };

    ctx.lineJoin = "round";
    ctx.lineCap = "round";
    // Contorno oscuro para que se vea sobre cualquier mapa
    ctx.strokeStyle = "rgba(0, 0, 0, 0.6)";
    ctx.lineWidth = grosor + 3;
    trazar();
    ctx.stroke();
    triangulo();
    ctx.stroke();

    ctx.strokeStyle = rgbCss(color);
    ctx.fillStyle = rgbCss(color);
    ctx.lineWidth = grosor;
    trazar();
    ctx.stroke();
    triangulo();
    ctx.fill();

    // Total de casillas recorridas, al lado de la punta
    const total = String(recorrido.length - 1);
    const tamanoLetra = Math.round(Math.max(13, Math.min(32, casillaPx * 0.5)));
    ctx.font = `bold ${tamanoLetra}px system-ui, sans-serif`;
    ctx.textAlign = "left";
    ctx.textBaseline = "top";
    const tx = fin.x + casillaPx * 0.3;
    const ty = fin.y + casillaPx * 0.3;
    ctx.lineWidth = 4;
    ctx.strokeStyle = "rgba(0, 0, 0, 0.8)";
    ctx.strokeText(total, tx, ty);
    ctx.fillStyle = "#fff";
    ctx.fillText(total, tx, ty);
  }

  ctx.restore();
}
