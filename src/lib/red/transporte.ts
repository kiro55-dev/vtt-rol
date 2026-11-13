// src/lib/red/transporte.ts
//
// Por dónde habla el JUGADOR con el host, sin que `conexion.svelte.ts` sepa si es el WebSocket (el túnel, la dirección
// propia o la red local) o la conexión directa de WebRTC (`webrtc.ts`). Espejo de `red/transporte.rs`.

export interface TransporteCliente {
  /** Un mensaje del protocolo (JSON). */
  enviarTexto(texto: string): void;
  /** Un trozo de imagen (de una subida). */
  enviarBinario(trozo: Uint8Array<ArrayBuffer>): void;
  /** Un mensaje que puede perderse o llegar desordenado (el arrastre en vivo). Por el WebSocket va como texto normal. */
  enviarEfimero(texto: string): void;
  /** Bytes aún sin enviar: para no llenar la conexión al subir una imagen. */
  pendiente(): number;
  abierto(): boolean;
  cerrar(): void;
}

/** Lo que recibe quien usa un transporte. */
export interface OyentesTransporte {
  alTexto(texto: string): void;
  alBinario(datos: ArrayBuffer): void;
  alCerrar(): void;
}

/** El WebSocket del host, como transporte. */
export function porWebSocket(url: string, oyentes: OyentesTransporte): TransporteCliente {
  const ws = new WebSocket(url);
  ws.binaryType = "arraybuffer";
  ws.onmessage = (e) => {
    if (typeof e.data === "string") oyentes.alTexto(e.data);
    else if (e.data instanceof ArrayBuffer) oyentes.alBinario(e.data);
  };
  ws.onclose = () => oyentes.alCerrar();
  const abierto = () => ws.readyState === WebSocket.OPEN;
  return {
    enviarTexto: (t) => abierto() && ws.send(t),
    enviarBinario: (b) => abierto() && ws.send(b),
    enviarEfimero: (t) => abierto() && ws.send(t),
    pendiente: () => ws.bufferedAmount,
    abierto,
    cerrar: () => ws.close()
  };
}
