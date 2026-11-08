// src/lib/fichasTablero.ts
// Fichas (instancias) en el tablero: cuáles hay en el mapa, cuál está bajo una casilla y cómo se
// dibujan. En el tablero todas son un cuadrado que ocupa justo las casillas de su cuerpo (girado si
// lo está), con un borde del color de su tipo (el mismo que la forma del banco y de «Fichas activas»).
// Las fichas con el foco dibujan además las formas de sus parámetros (rombo, círculo...).
import type { Casilla, DefinicionParametro, Forma, Instancia, TipoEntidad } from "$lib/partida.svelte";
import {
  extremosForma,
  ladosInstancia,
  mundoAPantalla,
  ocupa,
  rectanguloDe,
  type Camara,
  type Lados,
  type RejillaTablero
} from "$lib/tablero";

/** Color de cada tipo de ficha: Enemigo = cuadrado rojo, NPC = rombo azul, Jugador = círculo verde. */
export const COLOR_TIPO: Record<TipoEntidad, string> = {
  Enemigo: "#ef4444",
  NPC: "#3b82f6",
  Jugador: "#22c55e"
};

/** Instancia colocada con su casilla central. */
export type FichaEnMapa = { instancia: Instancia; casilla: Casilla };

/** Las instancias que están en una casilla de ese mapa, en orden de dibujo (la última, encima). */
export function fichasDelMapa(instancias: readonly Instancia[], idMapa: number | null): FichaEnMapa[] {
  if (idMapa === null) return [];
  return instancias.flatMap((instancia) =>
    instancia.id_mapa === idMapa && typeof instancia.coordenadas === "object"
      ? [{ instancia, casilla: instancia.coordenadas.Casilla }]
      : []
  );
}

/** La ficha que cubre una casilla (la de encima si se solapan). */
export function fichaEnCasilla(fichas: readonly FichaEnMapa[], casilla: Casilla): Instancia | null {
  for (let i = fichas.length - 1; i >= 0; i--) {
    const f = fichas[i];
    if (ocupa(f.casilla, ladosInstancia(f.instancia), casilla)) return f.instancia;
  }
  return null;
}

// ---------------------------------------------------------------------------
// Imágenes (a resolución completa: se ven grandes al hacer zoom)
// ---------------------------------------------------------------------------

const imagenes = new Map<string, HTMLImageElement>();

/** La imagen ya cargada de esa URL, o null mientras carga (al terminar se llama a `alCargar`). */
export function imagenDe(url: string, alCargar: () => void): HTMLImageElement | null {
  let img = imagenes.get(url);
  if (!img) {
    img = new Image();
    img.decoding = "async";
    img.onload = alCargar;
    img.src = url;
    imagenes.set(url, img);
  }
  return img.complete && img.naturalWidth > 0 ? img : null;
}

// ---------------------------------------------------------------------------
// Dibujo (en px de pantalla)
// ---------------------------------------------------------------------------

/** Rectángulo en pantalla de una ficha en su casilla. */
export function rectanguloFicha(cam: Camara, rejilla: RejillaTablero, ficha: FichaEnMapa) {
  const r = rectanguloDe(rejilla, ficha.casilla, ladosInstancia(ficha.instancia));
  const p = mundoAPantalla(cam, r.x, r.y);
  return { x: p.x, y: p.y, w: r.w * cam.zoom, h: r.h * cam.zoom };
}

/**
 * Dibuja una ficha. Por defecto en su casilla; con `en` (esquina superior izquierda en pantalla) en
 * otro sitio, p. ej. bajo el cursor mientras se arrastra (con `sombra`, como si se levantara).
 */
export function dibujarFicha(
  ctx: CanvasRenderingContext2D,
  cam: Camara,
  rejilla: RejillaTablero,
  ficha: FichaEnMapa,
  img: HTMLImageElement | null,
  opciones: { seleccionada?: boolean; en?: { x: number; y: number }; sombra?: boolean } = {}
) {
  const rect = rectanguloFicha(cam, rejilla, ficha);
  const { x, y } = opciones.en ?? rect;
  const { w, h } = rect;
  const borde = Math.max(2, Math.min(5, Math.min(w, h) * 0.06));
  const color = COLOR_TIPO[ficha.instancia.base.tipo] ?? "#a1a1aa";

  ctx.save();
  if (opciones.sombra) {
    ctx.shadowColor = "rgba(0, 0, 0, 0.55)";
    ctx.shadowBlur = 14;
    ctx.shadowOffsetX = 4;
    ctx.shadowOffsetY = 6;
  }
  ctx.fillStyle = "#18181b";
  ctx.fillRect(x, y, w, h);
  ctx.shadowColor = "transparent"; // la sombra, solo del cuadrado de fondo

  // La foto, girada como la ficha y recortada tipo "cover" dentro del cuadrado
  const giro = ((ficha.instancia.rotacion ?? 0) % 4) * (Math.PI / 2);
  const girada = (ficha.instancia.rotacion ?? 0) % 2 === 1;
  const [ancho, alto] = girada ? [h, w] : [w, h]; // tamaño antes de girar
  ctx.save();
  ctx.beginPath();
  ctx.rect(x, y, w, h);
  ctx.clip();
  ctx.translate(x + w / 2, y + h / 2);
  ctx.rotate(giro);
  if (img) {
    const escala = Math.max(ancho / img.naturalWidth, alto / img.naturalHeight);
    const sw = ancho / escala;
    const sh = alto / escala;
    ctx.drawImage(img, (img.naturalWidth - sw) / 2, (img.naturalHeight - sh) / 2, sw, sh, -ancho / 2, -alto / 2, ancho, alto);
  } else {
    // Sin foto: la inicial del nombre
    ctx.fillStyle = color;
    ctx.font = `bold ${Math.round(Math.min(w, h) * 0.5)}px system-ui, sans-serif`;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(ficha.instancia.base.nombre.charAt(0).toUpperCase(), 0, 0);
  }
  ctx.restore();

  ctx.lineWidth = borde;
  ctx.strokeStyle = color;
  ctx.strokeRect(x + borde / 2, y + borde / 2, w - borde, h - borde);

  if (opciones.seleccionada) {
    ctx.lineWidth = 2;
    ctx.strokeStyle = "#fafafa";
    ctx.setLineDash([6, 4]);
    ctx.strokeRect(x - 3, y - 3, w + 6, h + 6);
  }
  ctx.restore();
}

/** Rectángulo amarillo donde caerá la ficha que se está arrastrando. */
export function dibujarDestino(ctx: CanvasRenderingContext2D, cam: Camara, rejilla: RejillaTablero, casilla: Casilla, lados: Lados) {
  const r = rectanguloDe(rejilla, casilla, lados);
  const p = mundoAPantalla(cam, r.x, r.y);
  const w = r.w * cam.zoom;
  const h = r.h * cam.zoom;
  ctx.save();
  ctx.fillStyle = "rgba(250, 204, 21, 0.18)";
  ctx.strokeStyle = "rgba(250, 204, 21, 0.9)";
  ctx.lineWidth = 2;
  ctx.fillRect(p.x, p.y, w, h);
  ctx.strokeRect(p.x + 1, p.y + 1, w - 2, h - 2);
  ctx.restore();
}

// ---------------------------------------------------------------------------
// Formas de los parámetros (fichas con el foco)
// ---------------------------------------------------------------------------

const colorDe = (rgba: number[], alfa?: number) =>
  `rgba(${rgba[0] ?? 0}, ${rgba[1] ?? 0}, ${rgba[2] ?? 0}, ${alfa ?? (rgba[3] ?? 255) / 255})`;

/**
 * Dibuja, alrededor de la ficha, la forma de cada parámetro que la tenga. `valorDe(definicion)`
 * da el número que se usa (el actual si tiene máximo); `maximoDe` el máximo, para la barra de vida.
 *  - Cuadrado: el rectángulo cuyos lados están a `n` casillas de los bordes del cuerpo.
 *  - Rombo: une los cuatro puntos a `n` casillas (izquierda, arriba, derecha, abajo).
 *  - Círculo: la elipse (círculo con cuerpo cuadrado) que pasa por esos cuatro puntos.
 *  - Barra de vida: barra sobre la ficha con la proporción actual/máx.
 * Los cuatro puntos de referencia caen siempre en la rejilla; el resto de la figura, donde caiga.
 */
export function dibujarFormas(
  ctx: CanvasRenderingContext2D,
  cam: Camara,
  rejilla: RejillaTablero,
  ficha: FichaEnMapa,
  definiciones: readonly DefinicionParametro[],
  valorDe: (d: DefinicionParametro) => number,
  maximoDe: (d: DefinicionParametro) => number
) {
  const lados = ladosInstancia(ficha.instancia);
  // Pasa una línea de rejilla (en casillas) a pantalla
  const px = (columna: number) => cam.x + columna * rejilla.anchoCasilla * cam.zoom;
  const py = (fila: number) => cam.y + fila * rejilla.altoCasilla * cam.zoom;

  for (const d of definiciones) {
    const forma = d.representar_mapa;
    if (!forma || "Resumen" in forma) continue;
    const [tipo, rgba] = Object.entries(forma)[0] as [keyof Forma & string, number[]];

    if (tipo === "BarraVida") {
      dibujarBarra(ctx, rectanguloFicha(cam, rejilla, ficha), valorDe(d), maximoDe(d), rgba);
      continue;
    }

    const n = valorDe(d);
    if (!(n > 0)) continue;
    const e = extremosForma(ficha.casilla, lados, n);
    const [izq, der, arr, abj, cx, cy] = [px(e.izquierda), px(e.derecha), py(e.arriba), py(e.abajo), px(e.cx), py(e.cy)];

    ctx.save();
    ctx.beginPath();
    if (tipo === "Cuadrado") {
      ctx.rect(izq, arr, der - izq, abj - arr);
    } else if (tipo === "Rombo") {
      ctx.moveTo(izq, cy);
      ctx.lineTo(cx, arr);
      ctx.lineTo(der, cy);
      ctx.lineTo(cx, abj);
      ctx.closePath();
    } else if (tipo === "Circulo") {
      ctx.ellipse(cx, cy, (der - izq) / 2, (abj - arr) / 2, 0, 0, Math.PI * 2);
    }
    ctx.fillStyle = colorDe(rgba, Math.min(0.25, (rgba[3] ?? 90) / 255));
    ctx.fill();
    ctx.lineWidth = 2;
    ctx.strokeStyle = colorDe(rgba, 0.95);
    ctx.stroke();
    ctx.restore();
  }
}

function dibujarBarra(
  ctx: CanvasRenderingContext2D,
  ficha: { x: number; y: number; w: number; h: number },
  valor: number,
  maximo: number,
  rgba: number[]
) {
  const alto = Math.max(4, Math.min(10, ficha.h * 0.12));
  const y = ficha.y - alto - 3;
  const proporcion = maximo > 0 ? Math.min(1, Math.max(0, valor / maximo)) : 0;
  ctx.save();
  ctx.fillStyle = "rgba(0, 0, 0, 0.7)";
  ctx.fillRect(ficha.x, y, ficha.w, alto);
  ctx.fillStyle = colorDe(rgba, 1);
  ctx.fillRect(ficha.x + 1, y + 1, Math.max(0, (ficha.w - 2) * proporcion), alto - 2);
  ctx.restore();
}
