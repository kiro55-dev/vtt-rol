// src/lib/colorMarca.ts
//
// El color de las marcas de cada persona en el tablero (su flecha y su ping): tres canales 0–255. Funciones puras, con
// tests. Espejo de `ColorMarca` en src-tauri/src/tablero_general/tablero.rs.

export type ColorMarca = [number, number, number];

/** El verde de antes de que cada uno tuviera el suyo (el de los avisos sin color). */
export const COLOR_VERDE: ColorMarca = [34, 197, 94];

/** Un canal como entero de 0 a 255 (un valor que no es número se queda en 0). */
export function canal(v: number): number {
  return Number.isFinite(v) ? Math.min(255, Math.max(0, Math.round(v))) : 0;
}

export function limitarColor(c: readonly number[]): ColorMarca {
  return [canal(c[0]), canal(c[1]), canal(c[2])];
}

/** Color en HSL (tono 0–360, saturación y luminosidad 0–1) a RGB. */
export function hslARgb(h: number, s: number, l: number): ColorMarca {
  const k = (n: number) => (n + h / 30) % 12;
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) => l - a * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)));
  return limitarColor([f(0) * 255, f(8) * 255, f(4) * 255]);
}

/**
 * Un color al azar que se vea bien sobre un mapa (claro u oscuro): tono cualquiera, pero siempre vivo (saturación alta y
 * luminosidad media). `azar` devuelve un número entre 0 y 1 (solo se cambia en los tests).
 */
export function colorAleatorio(azar: () => number = Math.random): ColorMarca {
  return hslARgb(azar() * 360, 0.85, 0.55);
}

/** `#rrggbb` en minúsculas. */
export function aHex(c: readonly number[]): string {
  return "#" + limitarColor(c).map((x) => x.toString(16).padStart(2, "0")).join("");
}

/** De `#rgb`, `#rrggbb` (con o sin `#`) a color; `null` si no es un color. */
export function deHex(texto: string): ColorMarca | null {
  const t = texto.trim().replace(/^#/, "").toLowerCase();
  if (/^[0-9a-f]{3}$/.test(t)) return limitarColor([...t].map((x) => parseInt(x + x, 16)));
  if (/^[0-9a-f]{6}$/.test(t)) return limitarColor([0, 2, 4].map((i) => parseInt(t.slice(i, i + 2), 16)));
  return null;
}

/** El color como `rgb(r, g, b)` o `rgba(r, g, b, a)` para el canvas y los estilos. */
export function rgbCss(c: readonly number[], alfa?: number): string {
  const [r, g, b] = limitarColor(c);
  return alfa === undefined ? `rgb(${r}, ${g}, ${b})` : `rgba(${r}, ${g}, ${b}, ${alfa})`;
}

/** ¿Es un color válido (tres números de 0 a 255) lo que llega de fuera (un aviso o lo guardado)? */
export function esColor(c: unknown): c is ColorMarca {
  return Array.isArray(c) && c.length === 3 && c.every((x) => Number.isInteger(x) && x >= 0 && x <= 255);
}
