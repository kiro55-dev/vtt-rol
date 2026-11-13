// src/lib/red/webrtc.ts
//
// La conexión DIRECTA con el host (WebRTC, punto a punto), del lado del navegador (espejo de src-tauri/src/red/webrtc.rs).
// La señalización es automática (`senal.ts`, relés de Nostr): el jugador genera su oferta (cifrada con el secreto del
// enlace), la publica y recibe el código de respuesta del host. Antes de aceptar la respuesta se comprueba que su
// certificado es el del enlace (`f`): así nadie (ni un relé) puede hacerse pasar por el host.
//
// Canales (los abre el navegador): `juego` (fiable y ordenado: el protocolo, en trozos de ≤ 16 KiB), `imagenes`
// (fiable y ordenado) y `arrastre` (sin orden ni reintentos: el arrastre en vivo).
import { cifrar, descifrar, nonceAleatorio } from "./codigo";
import { huellaSdp, type Enlace } from "./enlace";
import type { OyentesTransporte, TransporteCliente } from "./transporte";
import { Recomponer, trocear } from "./trozos";

/** Lo máximo que ocupa un mensaje (igual que el host). */
const MAX_MENSAJE = 512 * 1024;
/** Para reunir los candidatos de la oferta (sin trickle: se manda todo de una vez). */
const ESPERA_CANDIDATOS_MS = 3000;
/** Un código de respuesta no vale pasados estos milisegundos. */
const EDAD_MAX_RESPUESTA_MS = 10 * 60 * 1000;
const STUN = ["stun:stun.l.google.com:19302", "stun:stun.cloudflare.com:3478"];

/** ¿Puede este navegador hacer la conexión directa? (WebRTC, WebCrypto y compresión: hace falta una página https.) */
export function soportaDirecto(): boolean {
  return (
    typeof RTCPeerConnection !== "undefined" &&
    typeof CompressionStream !== "undefined" &&
    typeof crypto !== "undefined" &&
    !!crypto.subtle &&
    (typeof isSecureContext === "undefined" || isSecureContext)
  );
}

/** Las huellas SHA-256 de un SDP (`a=fingerprint:sha-256 AA:BB:…`), en mayúsculas. */
export function huellasDelSdp(sdp: string): string[] {
  return [...sdp.matchAll(/^a=fingerprint:sha-256 ([0-9A-Fa-f:]+)\s*$/gim)].map((m) => m[1].toUpperCase());
}

export interface Directo {
  /** Mi oferta (cifrada), para publicarla en los relés. */
  codigo: string;
  /** El `n` de la oferta: la respuesta del host lleva el mismo. */
  n: string;
  /** El código que devuelve el host. Lanza un texto para enseñar si no vale (otra sala, vieja, de otro host…). */
  usarRespuesta(codigoRespuesta: string): Promise<void>;
  transporte: TransporteCliente;
}

/** Prepara la conexión directa: abre los tres canales y devuelve el código de la oferta. */
export async function crearDirecto(enlace: Enlace, oyentes: OyentesTransporte): Promise<Directo> {
  if (!enlace.huella) throw "Este enlace no ofrece conexión directa";
  const esperada = huellaSdp(enlace.huella);
  const pc = new RTCPeerConnection({ iceServers: [{ urls: STUN }] });
  const juego = pc.createDataChannel("juego");
  const imagenes = pc.createDataChannel("imagenes");
  const arrastre = pc.createDataChannel("arrastre", { ordered: false, maxRetransmits: 0 });
  for (const c of [juego, imagenes, arrastre]) c.binaryType = "arraybuffer";

  const recomponer = new Recomponer(MAX_MENSAJE);
  let cerrado = false;
  const cerrar = () => {
    if (cerrado) return;
    cerrado = true;
    pc.close();
    oyentes.alCerrar();
  };
  juego.onmessage = (e) => {
    try {
      const texto = recomponer.trozo(new Uint8Array(e.data as ArrayBuffer));
      if (texto !== null) oyentes.alTexto(texto);
    } catch {
      cerrar(); // un trozo que no vale: se corta
    }
  };
  imagenes.onmessage = (e) => oyentes.alBinario(e.data as ArrayBuffer);
  arrastre.onmessage = (e) => typeof e.data === "string" && oyentes.alTexto(e.data);
  for (const c of [juego, imagenes, arrastre]) c.onclose = cerrar;
  pc.onconnectionstatechange = () => {
    if (["failed", "closed", "disconnected"].includes(pc.connectionState)) cerrar();
  };

  await pc.setLocalDescription(await pc.createOffer());
  await new Promise<void>((ok) => {
    if (pc.iceGatheringState === "complete") return ok();
    const reloj = setTimeout(ok, ESPERA_CANDIDATOS_MS);
    pc.addEventListener("icegatheringstatechange", () => {
      if (pc.iceGatheringState === "complete") {
        clearTimeout(reloj);
        ok();
      }
    });
  });
  const sdp = pc.localDescription?.sdp;
  if (!sdp) throw "No se pudo preparar la conexión directa";
  const n = nonceAleatorio();
  const codigo = await cifrar(enlace.secreto, { tipo: "oferta", sdp, t: Date.now(), n });

  const abierto = () => juego.readyState === "open" && imagenes.readyState === "open";
  return {
    codigo,
    n,
    async usarRespuesta(codigoRespuesta: string) {
      const c = await descifrar(enlace.secreto, codigoRespuesta);
      if (c.tipo !== "respuesta") throw "Eso no es la respuesta del host (¿has pegado tu propio código?)";
      if (Math.abs(Date.now() - c.t) > EDAD_MAX_RESPUESTA_MS) throw "Esa respuesta es vieja: pídele al host otra";
      // Lo importante: que el certificado sea el del host del enlace
      const huellas = huellasDelSdp(c.sdp);
      if (huellas.length === 0 || huellas.some((h) => h !== esperada)) throw "Esta respuesta no es de tu host: no se conecta";
      await pc.setRemoteDescription({ type: "answer", sdp: c.sdp });
    },
    transporte: {
      enviarTexto(texto) {
        if (!abierto()) return;
        for (const t of trocear(texto)) juego.send(t);
      },
      enviarBinario(trozo) {
        if (abierto()) imagenes.send(trozo);
      },
      enviarEfimero(texto) {
        if (arrastre.readyState === "open") arrastre.send(texto);
      },
      pendiente: () => juego.bufferedAmount + imagenes.bufferedAmount,
      abierto,
      cerrar
    }
  };
}
