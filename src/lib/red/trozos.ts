// src/lib/red/trozos.ts
//
// Los mensajes del protocolo (JSON) por el canal `juego` de WebRTC (espejo de src-tauri/src/red/trozos.rs): cada uno
// se parte en trozos binarios de ≤ 16 KiB: 1 byte (`SIGUE` o `ULTIMO`) + un pedazo del texto en UTF-8. El canal es
// fiable y ordenado, así que basta con juntarlos hasta el último.

export const TROZO = 16 * 1024;
export const SIGUE = 0;
export const ULTIMO = 1;

export function trocear(texto: string): Uint8Array<ArrayBuffer>[] {
  const datos = new TextEncoder().encode(texto);
  if (datos.length === 0) return [new Uint8Array([ULTIMO])];
  const trozos: Uint8Array<ArrayBuffer>[] = [];
  for (let i = 0; i < datos.length; i += TROZO - 1) {
    const parte = datos.subarray(i, i + TROZO - 1);
    const t = new Uint8Array(parte.length + 1);
    t[0] = i + TROZO - 1 >= datos.length ? ULTIMO : SIGUE;
    t.set(parte, 1);
    trozos.push(t);
  }
  return trozos;
}

/** Junta trozos hasta tener un mensaje entero. `trozo` lanza si no vale o se pasa de `max`. */
export class Recomponer {
  #partes: Uint8Array[] = [];
  #total = 0;
  constructor(private max: number) {}

  trozo(t: Uint8Array): string | null {
    if (t.length === 0 || (t[0] !== SIGUE && t[0] !== ULTIMO)) throw new Error("trozo no válido");
    const datos = t.subarray(1);
    if (this.#total + datos.length > this.max) throw new Error("mensaje demasiado grande");
    this.#partes.push(datos.slice());
    this.#total += datos.length;
    if (t[0] === SIGUE) return null;
    const entero = new Uint8Array(this.#total);
    let i = 0;
    for (const p of this.#partes) {
      entero.set(p, i);
      i += p.length;
    }
    this.#partes = [];
    this.#total = 0;
    return new TextDecoder("utf-8", { fatal: true }).decode(entero);
  }
}
