// experimentos/fase4-nostr/probar.ts
//
// Prueba relés públicos de Nostr como buzón de la señalización: ¿aceptan un evento efímero (kind 27184) de ~2 KB con
// una clave recién inventada, y se lo entregan a quien lo espera por una etiqueta? Mide cuánto tardan.
//
//   bun run experimentos/fase4-nostr/probar.ts [wss://relé ...]
import { schnorr } from "@noble/curves/secp256k1.js";
import { sha256 } from "@noble/hashes/sha2.js";
import { bytesToHex, utf8ToBytes } from "@noble/hashes/utils.js";

const KIND = 27184;
const CANDIDATOS = [
  "wss://relay.damus.io",
  "wss://nos.lol",
  "wss://relay.primal.net",
  "wss://nostr.mom",
  "wss://offchain.pub",
  "wss://relay.nostr.band",
  "wss://nostr.wine",
  "wss://relay.snort.social",
  "wss://purplepag.es",
  "wss://relay.nostr.bg",
  "wss://nostr-pub.wellorder.net",
  "wss://relay.mostr.pub",
  "wss://nostr.oxtr.dev",
  "wss://relay.current.fyi"
];
const TOPE_MS = 8000;

function evento(contenido: string, etiqueta: string) {
  const sk = schnorr.keygen().secretKey;
  const pubkey = bytesToHex(schnorr.getPublicKey(sk));
  const created_at = Math.floor(Date.now() / 1000);
  const tags = [["t", etiqueta]];
  const id = bytesToHex(sha256(utf8ToBytes(JSON.stringify([0, pubkey, created_at, KIND, tags, contenido]))));
  const sig = bytesToHex(schnorr.sign(Uint8Array.from(Buffer.from(id, "hex")), sk));
  return { id, pubkey, created_at, kind: KIND, tags, content: contenido, sig };
}

function abrir(url: string): Promise<WebSocket> {
  return new Promise((ok, mal) => {
    const ws = new WebSocket(url);
    const reloj = setTimeout(() => {
      ws.close();
      mal(new Error("no conecta en " + TOPE_MS + " ms"));
    }, TOPE_MS);
    ws.onopen = () => {
      clearTimeout(reloj);
      ok(ws);
    };
    ws.onerror = () => {
      clearTimeout(reloj);
      mal(new Error("error al conectar"));
    };
  });
}

interface Resultado {
  url: string;
  ok: boolean;
  conexion_ms?: number;
  entrega_ms?: number;
  motivo?: string;
  avisos: string[];
}

async function probar(url: string, bytes: number): Promise<Resultado> {
  const r: Resultado = { url, ok: false, avisos: [] };
  const etiqueta = bytesToHex(crypto.getRandomValues(new Uint8Array(16)));
  const contenido = Buffer.from(crypto.getRandomValues(new Uint8Array(bytes))).toString("base64url");
  let oyente: WebSocket | undefined;
  let emisor: WebSocket | undefined;
  try {
    const t0 = performance.now();
    oyente = await abrir(url);
    emisor = await abrir(url);
    r.conexion_ms = Math.round(performance.now() - t0);

    const ev = evento(contenido, etiqueta);
    let publicado = false;
    const entregado = new Promise<number>((ok, mal) => {
      const reloj = setTimeout(() => mal(new Error("no llegó el evento")), TOPE_MS);
      oyente!.onmessage = (m) => {
        const d = JSON.parse(String(m.data));
        if (d[0] === "NOTICE" || d[0] === "CLOSED") r.avisos.push(String(d[d.length - 1]));
        if (d[0] === "AUTH") r.avisos.push("pide AUTH");
        if (d[0] === "EVENT" && d[2]?.id === ev.id) {
          clearTimeout(reloj);
          ok(performance.now());
        }
      };
    });
    entregado.catch(() => {});
    const respuestaOk = new Promise<string>((ok) => {
      emisor!.onmessage = (m) => {
        const d = JSON.parse(String(m.data));
        if (d[0] === "OK") ok(d[2] ? "" : String(d[3]));
        if (d[0] === "NOTICE" || d[0] === "AUTH") ok("rechaza: " + (d[1] ?? "AUTH"));
      };
      setTimeout(() => ok("sin OK"), TOPE_MS);
    });

    // El oyente se suscribe y espera al fin de lo almacenado antes de que se publique
    await new Promise<void>((listo) => {
      const prev = oyente!.onmessage;
      oyente!.send(JSON.stringify(["REQ", "s", { kinds: [KIND], "#t": [etiqueta], since: Math.floor(Date.now() / 1000) - 5 }]));
      const reloj = setTimeout(listo, 1500);
      const antes = oyente!.onmessage;
      oyente!.addEventListener("message", (m) => {
        const d = JSON.parse(String(m.data));
        if (d[0] === "EOSE") {
          clearTimeout(reloj);
          listo();
        }
      });
      void prev;
      void antes;
    });

    const t1 = performance.now();
    emisor.send(JSON.stringify(["EVENT", ev]));
    publicado = true;
    const motivoOk = await respuestaOk;
    if (motivoOk) {
      r.motivo = motivoOk;
      return r;
    }
    const llegada = await entregado;
    void publicado;
    r.entrega_ms = Math.round(llegada - t1);
    r.ok = true;
  } catch (e) {
    r.motivo = String((e as Error).message ?? e);
  } finally {
    oyente?.close();
    emisor?.close();
  }
  return r;
}

const reles = process.argv.slice(2).length ? process.argv.slice(2) : CANDIDATOS;
const bytes = 3000;
console.log(`Probando ${reles.length} relés con un evento efímero kind ${KIND} de ~${bytes} bytes\n`);
const resultados = await Promise.all(reles.map((u) => probar(u, bytes)));
for (const r of resultados) {
  const estado = r.ok ? "OK  " : "FALLA";
  const tiempos = r.ok ? `conexión ${r.conexion_ms} ms, entrega ${r.entrega_ms} ms` : (r.motivo ?? "?");
  console.log(`${estado} ${r.url.padEnd(34)} ${tiempos}${r.avisos.length ? "  [" + r.avisos.join("; ") + "]" : ""}`);
}
const buenos = resultados.filter((r) => r.ok).sort((a, b) => a.entrega_ms! - b.entrega_ms!);
console.log("\nFuncionan:", buenos.map((r) => r.url).join(", ") || "ninguno");
process.exit(0);
