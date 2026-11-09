<!-- cliente-web/src/TableroJugador.svelte -->
<!-- El mapa que enseña el host, con sus fichas, dibujado con lo mismo que el tablero de la app
     (src/lib/tablero.ts, fichasTablero.ts, marcasTablero.ts). De momento solo se mira:
     arrastrar = mover la vista, rueda o pellizco = zoom, tocar una ficha = ver sus formas (solo aquí). -->
<script lang="ts">
  import { onMount } from "svelte";
  import type { Instancia } from "$lib/partida.svelte";
  import type { AvisoTablero } from "$lib/avisosTablero";
  import { pantallaAMundo, rejillaDe, type Camara } from "$lib/tablero";
  import { dibujarFicha, dibujarFormas, fichaEnPunto, fichasDelMapa, imagenDe } from "$lib/fichasTablero";
  import { DURACION_PING_MS, dibujarPing, dibujarRecorrido, pingVivo, type Ping } from "$lib/marcasTablero";
  import type { Conexion } from "./conexion.svelte";
  import { urlImagen } from "./enlace";
  import { estaCaida, maximo, valorActual } from "./vista";

  let { conexion }: { conexion: Conexion } = $props();

  let contenedor = $state<HTMLDivElement | null>(null);
  let lienzo = $state<HTMLCanvasElement | null>(null);
  let ancho = $state(0);
  let alto = $state(0);
  let cam = $state<Camara>({ x: 0, y: 0, zoom: 1 });
  let enfocada = $state<number | null>(null);
  let recorrido = $state.raw<{ x: number; y: number }[]>([]);
  const pings: Ping[] = [];
  let version = $state(0); // se sube al cargar una imagen para repintar

  const vista = $derived(conexion.vista);
  const mapa = $derived(vista.mapa);
  const rejilla = $derived(mapa ? rejillaDe(mapa.ancho, mapa.alto, mapa.rejilla) : null);
  const fichas = $derived(fichasDelMapa(vista.instancias, mapa?.id ?? null));

  const repintar = () => version++;
  const url = (ruta: string, mini?: "ficha" | "mapa") =>
    conexion.sesion ? urlImagen(conexion.enlace, conexion.sesion, ruta, mini) : null;

  function imagenFicha(i: Instancia): HTMLImageElement | null {
    const u = i.base.miniatura_url ? url(i.base.miniatura_url) : i.base.foto_url ? url(i.base.foto_url, "ficha") : null;
    return u ? imagenDe(u, repintar) : null;
  }

  // Al cambiar de mapa, se encaja entero en la pantalla
  let mapaEncajado: number | null = null;
  $effect(() => {
    if (!mapa || !ancho || !alto || mapaEncajado === mapa.id) return;
    mapaEncajado = mapa.id;
    const zoom = Math.min(ancho / mapa.ancho, alto / mapa.alto) * 0.95;
    cam = { zoom, x: (ancho - mapa.ancho * zoom) / 2, y: (alto - mapa.alto * zoom) / 2 };
    recorrido = [];
  });

  // ---- Avisos del host (ping, recorrido) ----
  onMount(() => {
    conexion.alRecibirAviso((a: AvisoTablero) => {
      if (!mapa) return;
      if ("Ping" in a && a.Ping.id_mapa === mapa.id) {
        pings.push({ casilla: a.Ping.casilla, inicio: performance.now() });
        animar();
      } else if ("Recorrido" in a && a.Recorrido.id_mapa === mapa.id) {
        recorrido = a.Recorrido.casillas;
      } else if ("RecorridoBorrado" in a && a.RecorridoBorrado.id_mapa === mapa.id) {
        recorrido = [];
      }
    });
    const obs = new ResizeObserver(([e]) => {
      ancho = Math.floor(e.contentRect.width);
      alto = Math.floor(e.contentRect.height);
    });
    if (contenedor) obs.observe(contenedor);
    return () => obs.disconnect();
  });

  let animando = false;
  function animar() {
    if (animando) return;
    animando = true;
    const paso = () => {
      const ahora = performance.now();
      for (let i = pings.length - 1; i >= 0; i--) if (!pingVivo(pings[i], ahora)) pings.splice(i, 1);
      dibujar();
      if (pings.length > 0) requestAnimationFrame(paso);
      else animando = false;
    };
    requestAnimationFrame(paso);
    setTimeout(() => dibujar(), DURACION_PING_MS + 50);
  }

  // ---- Dibujo ----
  function dibujar() {
    const ctx = lienzo?.getContext("2d");
    if (!ctx || !lienzo) return;
    const dpr = window.devicePixelRatio || 1;
    if (lienzo.width !== ancho * dpr || lienzo.height !== alto * dpr) {
      lienzo.width = ancho * dpr;
      lienzo.height = alto * dpr;
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, ancho, alto);
    if (!mapa || !rejilla) return;

    // Fondo
    const fondo = mapa.url_imagen_fondo ? url(mapa.url_imagen_fondo) : null;
    const img = fondo ? imagenDe(fondo, repintar) : null;
    if (img) ctx.drawImage(img, cam.x, cam.y, mapa.ancho * cam.zoom, mapa.alto * cam.zoom);
    else {
      ctx.fillStyle = "#18181b";
      ctx.fillRect(cam.x, cam.y, mapa.ancho * cam.zoom, mapa.alto * cam.zoom);
    }

    // Rejilla (si las casillas se ven lo bastante grandes)
    const [r, g, b, a] = mapa.color_opacidad;
    if (rejilla.anchoCasilla * cam.zoom >= 6 && mapa.grosor > 0) {
      ctx.strokeStyle = `rgba(${r},${g},${b},${(a ?? 76) / 255})`;
      ctx.lineWidth = mapa.grosor;
      ctx.beginPath();
      for (let c = 0; c <= rejilla.columnas; c++) {
        const x = cam.x + c * rejilla.anchoCasilla * cam.zoom;
        ctx.moveTo(x, cam.y);
        ctx.lineTo(x, cam.y + mapa.alto * cam.zoom);
      }
      for (let f = 0; f <= rejilla.filas; f++) {
        const y = cam.y + f * rejilla.altoCasilla * cam.zoom;
        ctx.moveTo(cam.x, y);
        ctx.lineTo(cam.x + mapa.ancho * cam.zoom, y);
      }
      ctx.stroke();
    }

    // Formas de la ficha tocada (debajo de las fichas)
    const tocada = fichas.find((f) => f.instancia.id_instancia === enfocada);
    if (tocada) {
      const i = tocada.instancia;
      const defs = vista.definiciones.filter((d) => i.base.parametros.some((p) => p.id_definicion === d.id));
      dibujarFormas(ctx, cam, rejilla, tocada, defs, (d) => (d.tiene_maximo ? valorActual(i, d.id) : maximo(i, d.id)), (d) => maximo(i, d.id));
    }

    for (const f of fichas) {
      dibujarFicha(ctx, cam, rejilla, f, imagenFicha(f.instancia), {
        seleccionada: f.instancia.id_instancia === enfocada,
        apagada: estaCaida(f.instancia, vista.definiciones)
      });
    }

    dibujarRecorrido(ctx, cam, rejilla, recorrido);
    const ahora = performance.now();
    for (const p of pings) dibujarPing(ctx, cam, rejilla, p, ahora);
  }

  $effect(() => {
    void [vista, cam, ancho, alto, enfocada, recorrido, version, conexion.sesion];
    dibujar();
  });

  // ---- Mover la vista y zoom (ratón y dedos) ----
  const punteros = new Map<number, { x: number; y: number }>();
  let pulsado: { x: number; y: number } | null = null;
  let distanciaPellizco = 0;

  function zoomEn(px: number, py: number, factor: number) {
    const zoom = Math.min(8, Math.max(0.05, cam.zoom * factor));
    const k = zoom / cam.zoom;
    cam = { zoom, x: px - (px - cam.x) * k, y: py - (py - cam.y) * k };
  }

  function relativo(e: PointerEvent | WheelEvent) {
    const r = contenedor!.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  }

  function alPulsar(e: PointerEvent) {
    contenedor?.setPointerCapture(e.pointerId);
    punteros.set(e.pointerId, relativo(e));
    pulsado = punteros.size === 1 ? relativo(e) : null;
    if (punteros.size === 2) {
      const [a, b] = [...punteros.values()];
      distanciaPellizco = Math.hypot(a.x - b.x, a.y - b.y);
    }
  }

  function alMover(e: PointerEvent) {
    const antes = punteros.get(e.pointerId);
    if (!antes) return;
    const ahora = relativo(e);
    punteros.set(e.pointerId, ahora);
    if (punteros.size === 2) {
      const [a, b] = [...punteros.values()];
      const d = Math.hypot(a.x - b.x, a.y - b.y);
      if (distanciaPellizco > 0) zoomEn((a.x + b.x) / 2, (a.y + b.y) / 2, d / distanciaPellizco);
      distanciaPellizco = d;
      return;
    }
    if (pulsado && Math.hypot(ahora.x - pulsado.x, ahora.y - pulsado.y) > 5) pulsado = null;
    cam = { ...cam, x: cam.x + ahora.x - antes.x, y: cam.y + ahora.y - antes.y };
  }

  function alSoltar(e: PointerEvent) {
    punteros.delete(e.pointerId);
    if (pulsado && rejilla) {
      // Un toque sin moverse: la ficha de debajo enseña (o deja de enseñar) sus formas
      const p = pantallaAMundo(cam, pulsado.x, pulsado.y);
      const ficha = fichaEnPunto(fichas, rejilla, p.x, p.y);
      enfocada = ficha && ficha.id_instancia !== enfocada ? ficha.id_instancia : null;
    }
    pulsado = null;
  }
</script>

<div
  class="tablero"
  bind:this={contenedor}
  role="application"
  aria-label="Tablero"
  onpointerdown={alPulsar}
  onpointermove={alMover}
  onpointerup={alSoltar}
  onpointercancel={(e) => punteros.delete(e.pointerId)}
  onwheel={(e) => {
    e.preventDefault();
    const p = relativo(e);
    zoomEn(p.x, p.y, e.deltaY < 0 ? 1.15 : 1 / 1.15);
  }}
>
  <canvas bind:this={lienzo} style="width: {ancho}px; height: {alto}px;"></canvas>
  {#if !mapa}
    <p class="sin-mapa">El host no está enseñando ningún mapa ahora mismo.</p>
  {/if}
</div>

<style>
  .tablero { position: absolute; inset: 0; overflow: hidden; touch-action: none; cursor: grab; background: #09090b; }
  .tablero:active { cursor: grabbing; }
  canvas { display: block; }
  .sin-mapa { position: absolute; inset: 0; margin: auto; height: fit-content; text-align: center; color: #71717a; font-size: 14px; }
</style>
