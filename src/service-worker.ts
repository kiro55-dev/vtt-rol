// src/service-worker.ts
//
// Solo para la página de los JUGADORES (lo registra src/lib/red/conexion.svelte.ts; en la app del host nunca se
// registra). Hace UNA cosa: contestar las imágenes de la partida, `<base>/__imagen?ruta=…&mini=…`, pidiéndoselas a
// la página que las quiere, que las saca por su conexión con el host (`ImagenesRed`). Así un `<img>` o un `fetch`
// normales sirven aunque las imágenes no vengan por HTTP, y sin CORS (es el mismo origen).
//
// No guarda NADA de la app en caché (ni siquiera se entera de esas peticiones): una página vieja guardada fue la
// causa de la «pantalla negra» al volver a entrar.
/// <reference types="@sveltejs/kit" />
/// <reference no-default-lib="true"/>
/// <reference lib="esnext" />
/// <reference lib="webworker" />
import { base } from "$service-worker";

const sw = self as unknown as ServiceWorkerGlobalScope;
/** Lo que se espera a que la página entregue una imagen. */
const ESPERA_MS = 120_000;

sw.addEventListener("install", () => {
  void sw.skipWaiting(); // la versión nueva toma el relevo sin esperar a que se cierren las pestañas
});

sw.addEventListener("activate", (e) => {
  e.waitUntil(sw.clients.claim()); // y controla ya la página abierta (si no, sus <img> no pasarían por aquí)
});

sw.addEventListener("fetch", (e) => {
  const url = new URL(e.request.url);
  if (e.request.method !== "GET" || url.origin !== sw.location.origin || url.pathname !== `${base}/__imagen`) return;
  e.respondWith(pedirALaPagina(e.clientId, url));
});

async function pedirALaPagina(idCliente: string, url: URL): Promise<Response> {
  const cliente = (idCliente && (await sw.clients.get(idCliente))) || (await sw.clients.matchAll({ type: "window" }))[0];
  if (!cliente) return new Response(null, { status: 503 });
  const canal = new MessageChannel();
  const respuesta = new Promise<{ ok: boolean; blob?: Blob }>((ok) => {
    const reloj = setTimeout(() => ok({ ok: false }), ESPERA_MS);
    canal.port1.onmessage = (m) => {
      clearTimeout(reloj);
      ok(m.data);
    };
  });
  cliente.postMessage(
    { tipo: "openroll-imagen", ruta: url.searchParams.get("ruta"), mini: url.searchParams.get("mini") },
    [canal.port2]
  );
  const r = await respuesta;
  if (!r.ok || !r.blob) return new Response(null, { status: 404 });
  return new Response(r.blob, {
    headers: { "Content-Type": r.blob.type || "application/octet-stream", "X-Content-Type-Options": "nosniff" }
  });
}
