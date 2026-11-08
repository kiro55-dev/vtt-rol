// src/lib/tablero.ts
// Lógica del tablero en coordenadas de CASILLA, como una hoja de cálculo: (0,0) es la casilla de
// arriba a la izquierda, (1,0) la de su derecha y (0,1) la de debajo. Funciones puras.
//
// La parte en casillas (rejillaDe, tamanoCuerpo, extension, cabe, encajar, casillasOcupadas) replica
// src-tauri/src/tablero_general/tablero.rs con la misma aritmética entera: si cambias una, cambia la
// otra y sus tests. Lo de píxeles (cámara, ratón, rectángulos de dibujo) solo existe aquí.
//
// Rejilla: el mapa se divide en un número ENTERO de columnas y filas, redondeando el tamaño pedido
// para que las casillas encajen justas en los bordes: columnas = max(1, round(ancho / tamano)).
//
// Cuerpo: una ficha ocupa un rectángulo de `lados` casillas y su posición es la casilla CENTRAL
// (con un lado par, la de arriba/izquierda de las dos centrales). Un giro futuro solo intercambiaría
// los lados que se pasan a estas funciones.
import type { Casilla, Cuerpo } from "$lib/partida.svelte";

/** Rejilla de un mapa: cuántas casillas tiene y cuánto mide cada una en px del mapa. */
export interface RejillaTablero {
  columnas: number;
  filas: number;
  anchoCasilla: number;
  altoCasilla: number;
  ancho: number; // px del mapa
  alto: number;
}

/** [ancho, alto] en casillas */
export type Lados = readonly [number, number];

/** Cámara del tablero: pantalla = mundo · zoom + (x, y). El mundo son los px del mapa. */
export interface Camara {
  x: number;
  y: number;
  zoom: number;
}

export interface Rectangulo {
  x: number;
  y: number;
  w: number;
  h: number;
}

const entero = (v: number, min = 0) => Math.max(min, Math.floor(Number(v) || 0));

/** max(1, round(lado / tamano)) en enteros, igual que `casillas_en` de Rust. */
function casillasEn(ladoPx: number, tamano: number): number {
  const t = entero(tamano, 1);
  return Math.max(1, Math.floor((2 * entero(ladoPx) + t) / (2 * t)));
}

export function rejillaDe(ancho: number, alto: number, tamano: number): RejillaTablero {
  const columnas = casillasEn(ancho, tamano);
  const filas = casillasEn(alto, tamano);
  return { columnas, filas, anchoCasilla: ancho / columnas, altoCasilla: alto / filas, ancho, alto };
}

export function tamanoCuerpo(cuerpo: Cuerpo | null | undefined): Lados {
  if (!cuerpo || typeof cuerpo !== "object") return [1, 1];
  const lado = (i: number) => Math.max(1, entero(cuerpo.Grande[i] ?? 1));
  return [lado(0), lado(1)];
}

/** Casillas antes y después de la central: 1 → [0,0], 2 → [0,1], 3 → [1,1], 4 → [1,2]. */
export function extension(lado: number): [number, number] {
  const resto = Math.max(1, entero(lado)) - 1;
  const antes = Math.floor(resto / 2);
  return [antes, resto - antes];
}

/** Casillas extremas (incluidas) del cuerpo. Pueden salirse del mapa, incluso a negativo. */
export function extremos(centro: Casilla, lados: Lados) {
  const [ax, dx] = extension(lados[0]);
  const [ay, dy] = extension(lados[1]);
  return { x0: centro.x - ax, y0: centro.y - ay, x1: centro.x + dx, y1: centro.y + dy };
}

/** Todo el cuerpo queda dentro del mapa. */
export function cabe(rejilla: RejillaTablero, centro: Casilla, lados: Lados): boolean {
  const { x0, y0, x1, y1 } = extremos(centro, lados);
  return x0 >= 0 && y0 >= 0 && x1 < rejilla.columnas && y1 < rejilla.filas;
}

function encajarEje(centro: number, lado: number, casillas: number): number {
  const [antes, despues] = extension(lado);
  const maximo = Math.max(antes, casillas - 1 - despues);
  return Math.min(maximo, Math.max(antes, centro));
}

/** Mueve el centro lo justo para que el cuerpo quepa; si es más grande que el mapa, arriba a la izquierda. */
export function encajar(rejilla: RejillaTablero, centro: Casilla, lados: Lados): Casilla {
  return {
    x: encajarEje(centro.x, lados[0], rejilla.columnas),
    y: encajarEje(centro.y, lados[1], rejilla.filas)
  };
}

/**
 * Dónde cae cada ficha al soltar varias a la vez: la primera en `casilla` y las demás a su derecha y
 * debajo, en una cuadrícula casi cuadrada (columnas = la menor c con c·c >= n). El paso es el cuerpo
 * más ancho y el más alto, así no se solapan. Igual que `disposicion_en_cuadricula` de Rust.
 */
export function disposicionEnCuadricula(rejilla: RejillaTablero, casilla: Casilla, lados: readonly Lados[]): Casilla[] {
  let columnas = 1;
  while (columnas * columnas < lados.length) columnas++;
  const pasoX = Math.max(1, ...lados.map((l) => Math.max(1, entero(l[0]))));
  const pasoY = Math.max(1, ...lados.map((l) => Math.max(1, entero(l[1]))));
  return lados.map((l, k) =>
    encajar(rejilla, { x: casilla.x + (k % columnas) * pasoX, y: casilla.y + Math.floor(k / columnas) * pasoY }, l)
  );
}

/**
 * Dónde cae cada ficha al arrastrar varias juntas `(dx, dy)` casillas: todas se mueven lo mismo y
 * conservan sus distancias; si alguna se saldría del mapa, el desplazamiento común se recorta lo justo.
 * Solo si el grupo no cabe en un eje, en ese eje cada una se encaja sola. Igual que `desplazar_grupo` de Rust.
 */
export function desplazarGrupo(
  rejilla: RejillaTablero,
  fichas: readonly { casilla: Casilla; lados: Lados }[],
  dx: number,
  dy: number
): Casilla[] {
  const recortar = (d: number, eje: "x" | "y", casillas: number) => {
    let minimo = -Infinity;
    let maximo = Infinity;
    for (const f of fichas) {
      const e = extremos(f.casilla, f.lados);
      const [a, b] = eje === "x" ? [e.x0, e.x1] : [e.y0, e.y1];
      minimo = Math.max(minimo, -a);
      maximo = Math.min(maximo, casillas - 1 - b);
    }
    return minimo <= maximo ? Math.min(maximo, Math.max(minimo, d)) : d;
  };
  const mx = recortar(dx, "x", rejilla.columnas);
  const my = recortar(dy, "y", rejilla.filas);
  return fichas.map((f) =>
    encajar(rejilla, { x: Math.max(0, f.casilla.x + mx), y: Math.max(0, f.casilla.y + my) }, f.lados)
  );
}

/** Casillas que cubre el cuerpo (sin las que caerían en negativo). */
export function casillasOcupadas(centro: Casilla, lados: Lados): Casilla[] {
  const { x0, y0, x1, y1 } = extremos(centro, lados);
  const casillas: Casilla[] = [];
  for (let y = Math.max(0, y0); y <= y1; y++) {
    for (let x = Math.max(0, x0); x <= x1; x++) casillas.push({ x, y });
  }
  return casillas;
}

/** La casilla pertenece al cuerpo (para saber qué ficha hay bajo el ratón). */
export function ocupa(centro: Casilla, lados: Lados, casilla: Casilla): boolean {
  const { x0, y0, x1, y1 } = extremos(centro, lados);
  return casilla.x >= x0 && casilla.x <= x1 && casilla.y >= y0 && casilla.y <= y1;
}

// ---------------------------------------------------------------------------
// Píxeles
// ---------------------------------------------------------------------------

export function pantallaAMundo(cam: Camara, px: number, py: number) {
  return { x: (px - cam.x) / cam.zoom, y: (py - cam.y) / cam.zoom };
}

export function mundoAPantalla(cam: Camara, x: number, y: number) {
  return { x: x * cam.zoom + cam.x, y: y * cam.zoom + cam.y };
}

/** Casilla bajo un punto del mapa (px del mapa); null si cae fuera. */
export function casillaEn(rejilla: RejillaTablero, x: number, y: number): Casilla | null {
  if (!(x >= 0 && y >= 0 && x < rejilla.ancho && y < rejilla.alto)) return null;
  return {
    x: Math.min(rejilla.columnas - 1, Math.floor(x / rejilla.anchoCasilla)),
    y: Math.min(rejilla.filas - 1, Math.floor(y / rejilla.altoCasilla))
  };
}

/** Área en px del mapa que cubre el cuerpo: donde se dibuja la ficha. */
export function rectanguloDe(rejilla: RejillaTablero, centro: Casilla, lados: Lados): Rectangulo {
  const { x0, y0, x1, y1 } = extremos(centro, lados);
  return {
    x: x0 * rejilla.anchoCasilla,
    y: y0 * rejilla.altoCasilla,
    w: (x1 - x0 + 1) * rejilla.anchoCasilla,
    h: (y1 - y0 + 1) * rejilla.altoCasilla
  };
}

/**
 * Casilla central donde queda un cuerpo soltado con el cursor en (x, y) px del mapa: el centro
 * del rectángulo sigue al cursor y luego se encaja dentro del mapa. Con un lado impar es la casilla
 * bajo el cursor; con uno par, la que deja el cursor más cerca del centro geométrico.
 */
export function origenAlSoltar(rejilla: RejillaTablero, x: number, y: number, lados: Lados): Casilla {
  const eje = (v: number, tam: number, lado: number) => Math.round(v / tam - lado / 2) + extension(lado)[0];
  const centro = { x: eje(x, rejilla.anchoCasilla, lados[0]), y: eje(y, rejilla.altoCasilla, lados[1]) };
  return encajar(rejilla, centro, lados);
}

// ---------------------------------------------------------------------------
// Recorridos (la flecha que se dibuja arrastrando con el botón derecho)
// ---------------------------------------------------------------------------

const iguales = (a: Casilla, b: Casilla) => a.x === b.x && a.y === b.y;

/** Casilla más cercana a un punto del mapa (px del mapa), aunque el punto caiga fuera. */
export function casillaMasCercana(rejilla: RejillaTablero, x: number, y: number): Casilla {
  const eje = (v: number, tam: number, n: number) => Math.min(n - 1, Math.max(0, Math.floor(v / tam)));
  return { x: eje(x, rejilla.anchoCasilla, rejilla.columnas), y: eje(y, rejilla.altoCasilla, rejilla.filas) };
}

/**
 * Casillas que se cruzan yendo de `a` a `b` paso a paso, solo en horizontal o vertical (sin `a`,
 * con `b`). Sirve para no saltarse casillas cuando el ratón se mueve deprisa.
 */
export function pasosEntre(a: Casilla, b: Casilla): Casilla[] {
  const nx = Math.abs(b.x - a.x);
  const ny = Math.abs(b.y - a.y);
  const sx = Math.sign(b.x - a.x);
  const sy = Math.sign(b.y - a.y);
  const pasos: Casilla[] = [];
  let { x, y } = a;
  for (let ix = 0, iy = 0; ix < nx || iy < ny; ) {
    // Avanza por el eje cuya siguiente frontera queda antes a lo largo de la recta a→b
    if ((0.5 + ix) * ny < (0.5 + iy) * nx) {
      x += sx;
      ix++;
    } else {
      y += sy;
      iy++;
    }
    pasos.push({ x, y });
  }
  return pasos;
}

/**
 * Añade al recorrido las casillas hasta `destino`. Volver a una casilla por la que ya pasó lo
 * recorta hasta ella: así, al deshacer el camino con el ratón, la flecha se acorta.
 * Las casillas recorridas son `recorrido.length - 1` (la primera es el punto de partida).
 */
export function extenderRecorrido(recorrido: readonly Casilla[], destino: Casilla): Casilla[] {
  if (recorrido.length === 0) return [destino];
  const nuevo = recorrido.slice();
  for (const paso of pasosEntre(nuevo[nuevo.length - 1], destino)) {
    const i = nuevo.findIndex((c) => iguales(c, paso));
    if (i === -1) nuevo.push(paso);
    else nuevo.length = i + 1;
  }
  return nuevo;
}

/** Centro de una casilla en px del mapa. */
export function centroDe(rejilla: RejillaTablero, c: Casilla) {
  return { x: (c.x + 0.5) * rejilla.anchoCasilla, y: (c.y + 0.5) * rejilla.altoCasilla };
}

// ---------------------------------------------------------------------------
// Instancias giradas y formas de sus parámetros
// ---------------------------------------------------------------------------

/** Lados de una instancia en el tablero: los de su cuerpo, intercambiados con un giro impar
 *  (igual que `tablero::lados_instancia` en Rust). */
export function ladosInstancia(inst: { base: { cuerpo: Cuerpo }; rotacion?: number }): Lados {
  const [ancho, alto] = tamanoCuerpo(inst.base.cuerpo);
  return (inst.rotacion ?? 0) % 2 === 1 ? [alto, ancho] : [ancho, alto];
}

/**
 * Límites (en casillas, como LÍNEAS de la rejilla) de la forma de un parámetro de valor `n` alrededor
 * de un cuerpo: se cuentan `n` casillas desde cada borde del cuerpo. Los cuatro puntos de referencia
 * (izquierda, derecha, arriba, abajo) caen justo en la rejilla; `cx`/`cy` es el centro del cuerpo.
 */
export function extremosForma(centro: Casilla, lados: Lados, n: number) {
  const { x0, y0, x1, y1 } = extremos(centro, lados);
  return {
    izquierda: x0 - n,
    derecha: x1 + 1 + n,
    arriba: y0 - n,
    abajo: y1 + 1 + n,
    cx: (x0 + x1 + 1) / 2,
    cy: (y0 + y1 + 1) / 2
  };
}
