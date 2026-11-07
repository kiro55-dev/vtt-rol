<!-- src/routes/componentes/Tablero.svelte -->
<script>
  import { onMount, untrack, onDestroy } from 'svelte';
  import { urlDeImagen } from '$lib/imagenes';
  import { rejillaDe, pantallaAMundo, casillaEn, casillaMasCercana, extenderRecorrido } from '$lib/tablero';
  import { dibujarPing, dibujarRecorrido, pingVivo } from '$lib/marcasTablero';
  import {
    alRecibirAviso, enviarAviso, avisoPing, avisoRecorrido, avisoRecorridoBorrado
  } from '$lib/avisosTablero';

  /** @type {{ mapa?: any, rejilla?: any }} */
  let {
    mapa = { ancho: 1000, alto: 1000, url_imagen_fondo: '' },
    rejilla = { visible: true, tamano: 40, color: '#ffffff', grosor: 1, opacidad: 0.3 }
  } = $props();

  // Por debajo de este tamaño en pantalla (px) la rejilla no se dibuja: no se ve y solo molesta.
  const PASO_MIN_PX = 5;
  const ZOOM_MIN = 0.1;
  const ZOOM_MAX = 8;

  // Elementos del DOM (solo se usan desde funciones, no hace falta que sean reactivos)
  /** @type {HTMLDivElement} */
  let contenedor;
  /** @type {HTMLCanvasElement} */
  let canvasFondo;
  /** @type {HTMLCanvasElement} */
  let canvasRejilla;
  /** @type {HTMLCanvasElement} */
  let canvasMarcas;

  // Tamaño visible del tablero (px CSS) y densidad de píxeles de la pantalla.
  let ancho = $state(0);
  let alto = $state(0);
  let dpr = $state(1);

  // Cámara: pantalla = mundo * zoom + (x, y). El "mundo" es el mapa en sus píxeles originales.
  // $state.raw: no se convierte en proxy; se actualiza sustituyendo el objeto entero.
  let cam = $state.raw({ x: 0, y: 0, zoom: 1 });

  // Un <img> del DOM no debe volverse proxy: $state.raw.
  let imagenFondo = $state.raw(null);
  let urlActual = null;
  /** @type {number | null} */
  let idActual = null; // id del mapa mostrado
  let tokenCarga = 0;

  let panActivo = $state(false);
  let ultimo = { x: 0, y: 0 };

  // Recorrido medido con el botón derecho: casillas desde la de salida. Se queda dibujado al soltar
  // y desaparece con el siguiente clic, sea donde sea. Cada cambio se avisa (ver $lib/avisosTablero).
  /** @type {import("$lib/partida.svelte").Casilla[]} */
  let recorrido = $state.raw([]);
  let midiendo = false;
  // Recorrido de otro jugador, recibido por aviso
  /** @type {import("$lib/partida.svelte").Casilla[]} */
  let recorridoAjeno = $state.raw([]);

  // Pings en curso (clic central). No es reactivo: los anima el bucle de pintado mientras duren.
  /** @type {import("$lib/marcasTablero").Ping[]} */
  const pings = [];
  // Pulsación del botón central: si se suelta casi sin moverse es un ping; si no, era mover la vista.
  /** @type {{ x: number, y: number } | null} */
  let pulsacionCentral = null;
  const MOVIMIENTO_MAX_CLIC_PX = 5;

  const clamp = (v, min, max) => Math.min(max, Math.max(min, v));

  // Tamaño del mapa en unidades de mundo: el que guarda Rust (el de su imagen de fondo), así las
  // casillas son las mismas aquí que allí. La imagen se estira a ese tamaño.
  function dimsMundo() {
    return { w: mapa?.ancho || 1000, h: mapa?.alto || 1000 };
  }

  // ---------------------------------------------------------------------------
  // Carga del fondo: se repite cuando cambia el mapa
  // ---------------------------------------------------------------------------
  $effect(() => {
    const url = mapa?.url_imagen_fondo || '';
    const id = mapa?.id ?? null;
    untrack(() => cargarFondo(url, id));
  });

  // La memoria solo se destruye al cerrar el tablero, no en cada cambio de mapa
  onDestroy(() => {
    liberarMemoriaImagen();
  });

  function liberarMemoriaImagen() {
    if (imagenFondo) {
      // Desvincular eventos primero para evitar bucle infinito de onerror en WebView2
      imagenFondo.onload = null;
      imagenFondo.onerror = null;
      imagenFondo.src = '';
      imagenFondo = null;
    }
  }

  /**
   * @param {string} url
   * @param {number | null} id
   */
  function cargarFondo(url, id) {
    const cambioDeMapa = id !== idActual;
    idActual = id;

    // Mismo fondo (ya cargado, o sin imagen): no se recarga nada. La cámara solo se recentra si es
    // OTRO mapa; si no, guardar la rejilla o renombrar mapas la devolvería al centro cada vez.
    if (url === urlActual && (imagenFondo || !url)) {
      if (cambioDeMapa) ajustarVista();
      return;
    }

    urlActual = url;
    const mio = ++tokenCarga;

    // Liberamos la imagen anterior limpiando sus listeners
    liberarMemoriaImagen();

    if (!url) {
      ajustarVista();
      return;
    }

    const img = new Image();
    img.decoding = 'async';

    img.onload = async () => {
      if (mio !== tokenCarga) {
        img.onload = null;
        img.onerror = null;
        img.src = '';
        return;
      }
      try {
        await img.decode();
      } catch {
        // Ignorar si decode() ya terminó
      }
      if (mio !== tokenCarga) {
        img.onload = null;
        img.onerror = null;
        img.src = '';
        return;
      }
      imagenFondo = img;
      ajustarVista();
    };

    img.onerror = () => {
      if (mio !== tokenCarga) return;
      img.onload = null;
      img.onerror = null;
      imagenFondo = null;
      ajustarVista();
    };

    img.src = urlDeImagen(url); // `url` es la ruta relativa guardada en el mapa
  }

  // ---------------------------------------------------------------------------
  // Cámara
  // ---------------------------------------------------------------------------
  function ajustarVista() {
    if (!ancho || !alto) return;
    const { w, h } = dimsMundo();
    const zoom = clamp(Math.min(ancho / w, alto / h) * 0.95, ZOOM_MIN, 1);
    cam = { zoom, x: (ancho - w * zoom) / 2, y: (alto - h * zoom) / 2 };
  }

  function alRueda(e) {
    e.preventDefault(); // en Svelte 5 onwheel no es pasivo, así que esto funciona
    const r = contenedor.getBoundingClientRect();
    const px = e.clientX - r.left;
    const py = e.clientY - r.top;
    const zoom = clamp(cam.zoom * Math.exp(-e.deltaY * 0.0015), ZOOM_MIN, ZOOM_MAX);
    const k = zoom / cam.zoom;
    // El punto del mundo bajo el cursor se queda bajo el cursor.
    cam = { zoom, x: px - (px - cam.x) * k, y: py - (py - cam.y) * k };
  }

  // Punto del mapa (px del mapa) bajo un evento del ratón
  /** @param {PointerEvent} e */
  function puntoDelMapa(e) {
    const r = contenedor.getBoundingClientRect();
    return pantallaAMundo(cam, e.clientX - r.left, e.clientY - r.top);
  }

  // Botones:
  //  * izquierdo: arrastrar mueve la vista. Cuando haya fichas: solo si el clic no cae en una.
  //  * central: clic = ping; arrastrar también mueve la vista.
  //  * derecho: arrastrar mide un recorrido por casillas.
  function alPresionar(e) {
    if (e.button === 2) {
      e.preventDefault();
      const p = puntoDelMapa(e);
      const salida = casillaEn(casillas, p.x, p.y);
      if (!salida) return; // se empieza a medir solo desde dentro del mapa
      cambiarRecorrido([salida]);
      midiendo = true;
      contenedor.setPointerCapture(e.pointerId);
      return;
    }
    if (e.button !== 0 && e.button !== 1) return;
    e.preventDefault();
    panActivo = true;
    ultimo = { x: e.clientX, y: e.clientY };
    pulsacionCentral = e.button === 1 ? { x: e.clientX, y: e.clientY } : null;
    contenedor.setPointerCapture(e.pointerId);
  }

  function alMover(e) {
    if (midiendo) {
      const p = puntoDelMapa(e);
      const siguiente = extenderRecorrido(recorrido, casillaMasCercana(casillas, p.x, p.y));
      if (siguiente.length !== recorrido.length || siguiente.at(-1) !== recorrido.at(-1)) cambiarRecorrido(siguiente);
      return;
    }
    if (!panActivo) return;
    if (
      pulsacionCentral &&
      Math.hypot(e.clientX - pulsacionCentral.x, e.clientY - pulsacionCentral.y) > MOVIMIENTO_MAX_CLIC_PX
    ) {
      pulsacionCentral = null; // ya es un arrastre, no un clic
    }
    cam = { ...cam, x: cam.x + e.clientX - ultimo.x, y: cam.y + e.clientY - ultimo.y };
    ultimo = { x: e.clientX, y: e.clientY };
  }

  function alSoltar(e) {
    if (midiendo && e.button === 2) {
      midiendo = false; // el recorrido se queda dibujado hasta el siguiente clic
    }
    if (pulsacionCentral && e.button === 1) {
      const p = puntoDelMapa(e);
      const casilla = casillaEn(casillas, p.x, p.y);
      if (casilla) {
        lanzarPing(casilla);
        if (idMapa() !== null) enviarAviso(avisoPing(idMapa(), casilla));
      }
    }
    pulsacionCentral = null;
    panActivo = false;
    if (contenedor?.hasPointerCapture?.(e.pointerId)) contenedor.releasePointerCapture(e.pointerId);
  }

  // Un pointercancel (p. ej. la ventana pierde el foco) no debe dejar un ping ni una medida a medias
  /** @param {PointerEvent} e */
  function alCancelar(e) {
    midiendo = false;
    pulsacionCentral = null;
    panActivo = false;
    if (contenedor?.hasPointerCapture?.(e.pointerId)) contenedor.releasePointerCapture(e.pointerId);
  }

  // Id del mapa abierto (null si no hay uno guardado aún): los avisos van ligados a un mapa
  function idMapa() {
    return typeof mapa?.id === 'number' ? mapa.id : null;
  }

  // Cambia el recorrido propio y avisa. `[]` = quitarlo.
  /** @param {import("$lib/partida.svelte").Casilla[]} casillas */
  function cambiarRecorrido(casillas) {
    recorrido = casillas;
    const id = idMapa();
    if (id === null) return;
    enviarAviso(casillas.length ? avisoRecorrido(id, casillas) : avisoRecorridoBorrado(id));
  }

  // Un aviso de otro jugador (llega por el evento "aviso-tablero"; ver $lib/avisosTablero)
  /** @param {import("$lib/avisosTablero").AvisoTablero} aviso */
  function alLlegarAviso(aviso) {
    if ('Ping' in aviso) {
      if (aviso.Ping.id_mapa === idMapa()) lanzarPing(aviso.Ping.casilla);
    } else if ('Recorrido' in aviso) {
      if (aviso.Recorrido.id_mapa === idMapa()) recorridoAjeno = aviso.Recorrido.casillas;
    } else if (aviso.RecorridoBorrado.id_mapa === idMapa()) {
      recorridoAjeno = [];
    }
  }

  /** @param {import("$lib/partida.svelte").Casilla} casilla */
  function lanzarPing(casilla) {
    pings.push({ casilla, inicio: performance.now() });
    marcasPendientes = true;
    programar();
  }

  // Cualquier clic, en el tablero o fuera de él, borra el recorrido que quedó dibujado.
  // Va en fase de captura: si el clic es otro botón derecho, se borra antes de empezar el nuevo.
  function alPulsarEnLaVentana() {
    if (!midiendo && recorrido.length) cambiarRecorrido([]);
  }

  // ---------------------------------------------------------------------------
  // Pintado: cada capa se redibuja solo cuando cambia lo suyo, y como mucho 1 vez por frame.
  // Los efectos solo anotan "qué hay que pintar"; el pintado real ocurre en requestAnimationFrame.
  // ---------------------------------------------------------------------------
  /** @type {{ fondo: any, rejilla: any }} */
  const pendiente = { fondo: null, rejilla: null };
  // Al abrir otro mapa, las marcas del anterior no tienen sentido: se quitan (y se avisa del recorrido propio).
  /** @type {number | null} */
  let mapaDeLasMarcas = null;
  $effect(() => {
    const id = idMapa();
    untrack(() => {
      if (id === mapaDeLasMarcas) return;
      if (recorrido.length && mapaDeLasMarcas !== null) enviarAviso(avisoRecorridoBorrado(mapaDeLasMarcas));
      mapaDeLasMarcas = id;
      recorrido = [];
      recorridoAjeno = [];
      pings.length = 0;
      midiendo = false;
    });
  });

  // Capa de marcas (pings y recorrido): se repinta si cambia algo suyo o mientras haya pings animándose
  let marcasPendientes = true;
  let rafId = 0;

  function programar() {
    if (!rafId) rafId = requestAnimationFrame(pintar);
  }

  // Depende de: imagen, mapa, cámara y tamaño del tablero.
  $effect(() => {
    const { w, h } = dimsMundo();
    pendiente.fondo = { img: imagenFondo, w, h, cam, ancho, alto, dpr };
    programar();
  });

  // Casillas del mapa (lo mismo que calcula Rust en tablero.rs). Se usa el tamaño que se está
  // editando, aunque aún no se haya guardado, para ver el cambio al momento.
  const casillas = $derived.by(() => {
    const { w, h } = dimsMundo();
    return rejillaDe(w, h, rejilla.tamano);
  });

  // Depende de la cámara, del tamaño del tablero, y de los ajustes de la rejilla.
  $effect(() => {
    pendiente.rejilla = {
      cam, ancho, alto, dpr, casillas,
      estilo: {
        visible: rejilla.visible,
        color: rejilla.color,
        grosor: rejilla.grosor,
        opacidad: rejilla.opacidad
      }
    };
    programar();
  });

  // Depende de la cámara, del tamaño del tablero, de las casillas y del recorrido.
  $effect(() => {
    void [cam, ancho, alto, dpr, casillas, recorrido, recorridoAjeno];
    marcasPendientes = true;
    programar();
  });

  function pintarMarcas() {
    const ctx = prepararCanvas(canvasMarcas, ancho, alto, dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0); // en px de pantalla
    dibujarRecorrido(ctx, cam, casillas, recorridoAjeno);
    dibujarRecorrido(ctx, cam, casillas, recorrido);

    const ahora = performance.now();
    for (let i = pings.length - 1; i >= 0; i--) {
      if (!pingVivo(pings[i], ahora)) pings.splice(i, 1);
    }
    for (const ping of pings) dibujarPing(ctx, cam, casillas, ping, ahora);
  }

  function pintar() {
    rafId = 0;
    if (!canvasFondo || !canvasRejilla || !canvasMarcas) return;

    const f = pendiente.fondo;
    if (f && f.ancho && f.alto) {
      pendiente.fondo = null;
      pintarFondo(f);
    }

    const r = pendiente.rejilla;
    if (r && r.ancho && r.alto) {
      pendiente.rejilla = null;
      const ctx = prepararCanvas(canvasRejilla, r.ancho, r.alto, r.dpr);
      ctx.setTransform(r.dpr, 0, 0, r.dpr, 0, 0); // la rejilla se dibuja en px de pantalla
      dibujarRejilla(ctx, r.cam, r.casillas, r.ancho, r.alto, r.estilo);
    }

    if ((marcasPendientes || pings.length) && ancho && alto) {
      marcasPendientes = false;
      pintarMarcas();
      if (pings.length) programar(); // siguiente fotograma de la animación
    }
  }

  // Ajusta el tamaño real del canvas a la pantalla (nítido en HiDPI) y lo limpia.
  function prepararCanvas(canvas, anchoCss, altoCss, densidad) {
    const w = Math.round(anchoCss * densidad);
    const h = Math.round(altoCss * densidad);
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w;
      canvas.height = h;
    }
    const ctx = canvas.getContext('2d');
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    return ctx;
  }

  function pintarFondo(f) {
    const ctx = prepararCanvas(canvasFondo, f.ancho, f.alto, f.dpr);
    ctx.setTransform(f.dpr * f.cam.zoom, 0, 0, f.dpr * f.cam.zoom, f.dpr * f.cam.x, f.dpr * f.cam.y);
    if (f.img) {
      ctx.imageSmoothingQuality = 'high';
      ctx.drawImage(f.img, 0, 0, f.w, f.h);
    } else {
      ctx.fillStyle = '#1e1e1e';
      ctx.fillRect(0, 0, f.w, f.h);
    }
  }

  /**
   * Dibuja la rejilla en coordenadas de pantalla, solo dentro del mapa y solo las líneas visibles.
   * Las líneas van en los bordes de las casillas (i · anchoCasilla), incluidos los del mapa.
   * Coste: nº de líneas en pantalla (nunca más de ~ancho/PASO_MIN_PX por eje), da igual el tamaño del mapa.
   */
  function dibujarRejilla(ctx, cam, casillas, vistaW, vistaH, estilo) {
    if (!estilo || estilo.visible === false) return;

    // Si el grosor es 0 o menor, no dibuja la rejilla
    const grosor = Number(estilo.grosor);
    if (!(grosor > 0)) return;

    const opacidad = clamp(Number(estilo.opacidad) || 0, 0, 1);
    if (opacidad <= 0) return;

    const pasoX = casillas.anchoCasilla * cam.zoom;
    const pasoY = casillas.altoCasilla * cam.zoom;
    if (Math.min(pasoX, pasoY) < PASO_MIN_PX) return;

    // Rectángulo del mapa en pantalla, recortado a lo que se ve.
    const izq = cam.x;
    const arriba = cam.y;
    const x0 = Math.max(izq, 0);
    const x1 = Math.min(izq + casillas.ancho * cam.zoom, vistaW);
    const y0 = Math.max(arriba, 0);
    const y1 = Math.min(arriba + casillas.alto * cam.zoom, vistaH);
    if (x0 >= x1 || y0 >= y1) return;

    // Con grosor impar, la línea va al centro del píxel para que no salga borrosa.
    const medio = Math.round(grosor) % 2 === 1 ? 0.5 : 0;

    ctx.save();
    ctx.strokeStyle = estilo.color;
    ctx.lineWidth = grosor;
    ctx.globalAlpha = opacidad;
    ctx.beginPath();

    // Índices de la primera y la última línea visibles en cada eje (hay columnas + 1 líneas).
    const iMin = Math.max(0, Math.ceil((x0 - izq) / pasoX));
    const iMax = Math.min(casillas.columnas, Math.floor((x1 - izq) / pasoX + 1e-9));
    for (let i = iMin; i <= iMax; i++) {
      const x = Math.round(izq + i * pasoX) + medio;
      ctx.moveTo(x, y0);
      ctx.lineTo(x, y1);
    }

    const jMin = Math.max(0, Math.ceil((y0 - arriba) / pasoY));
    const jMax = Math.min(casillas.filas, Math.floor((y1 - arriba) / pasoY + 1e-9));
    for (let j = jMin; j <= jMax; j++) {
      const y = Math.round(arriba + j * pasoY) + medio;
      ctx.moveTo(x0, y);
      ctx.lineTo(x1, y);
    }

    ctx.stroke();
    ctx.restore();
  }

  onMount(() => {
    const observador = new ResizeObserver(([entrada]) => {
      const primera = !ancho;
      dpr = window.devicePixelRatio || 1;
      ancho = Math.floor(entrada.contentRect.width);
      alto = Math.floor(entrada.contentRect.height);
      if (primera) ajustarVista();
    });
    observador.observe(contenedor);
    window.addEventListener('pointerdown', alPulsarEnLaVentana, true);
    const dejarDeEscuchar = alRecibirAviso(alLlegarAviso);

    return () => {
      observador.disconnect();
      window.removeEventListener('pointerdown', alPulsarEnLaVentana, true);
      dejarDeEscuchar();
      cancelAnimationFrame(rafId);
    };
  });
</script>

<div
  class="tablero"
  class:arrastrando={panActivo}
  bind:this={contenedor}
  role="application"
  aria-label="Tablero de juego"
  title="Rueda: zoom · Arrastrar: mover · Doble clic: centrar · Clic central: aviso · Arrastrar con botón derecho: medir"
  onwheel={alRueda}
  onpointerdown={alPresionar}
  onpointermove={alMover}
  onpointerup={alSoltar}
  onpointercancel={alCancelar}
  oncontextmenu={(e) => e.preventDefault()}
  ondblclick={() => ajustarVista()}
>
  <canvas bind:this={canvasFondo}></canvas>
  <canvas bind:this={canvasRejilla}></canvas>
  <canvas bind:this={canvasMarcas}></canvas>
</div>

<style>
  .tablero {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background-color: #121212;
    cursor: grab;
    touch-action: none;
  }
  .tablero.arrastrando {
    cursor: grabbing;
  }
  canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none; /* los eventos los gestiona el contenedor */
  }
</style>