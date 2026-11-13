<!-- src/routes/componentes/Tablero.svelte -->
<script>
  import { ayuda } from "$lib/ajustesVista.svelte";
  import { onMount, untrack, onDestroy } from 'svelte';
  import { urlDeImagen } from '$lib/imagenes';
  import {
    rejillaDe, pantallaAMundo, mundoAPantalla, casillaEn, casillaMasCercana, extenderRecorrido, origenAlSoltar,
    disposicionEnCuadricula, desplazarGrupo, ladosInstancia
  } from '$lib/tablero';
  import { partida, numeroDe, veParametro } from '$lib/partida.svelte';
  import { arrastre, baseDe, ladosDeEntidad, instanciasArrastradas } from '$lib/arrastreFicha.svelte';
  import { miniaturas } from '$lib/miniaturas.svelte';
  import { arrastreJugador } from '$lib/arrastreJugador.svelte';
  import { arrastreAjeno } from '$lib/arrastreAjeno.svelte';
  import {
    fichasDelMapa, fichaEnPunto, fichasEnRectangulo, imagenDe, dibujarFicha, dibujarDestino, dibujarFormas, rectanguloFicha
  } from '$lib/fichasTablero';
  import { dibujarPing, dibujarRecorrido, pingVivo } from '$lib/marcasTablero';
  import { ajustesVista } from '$lib/ajustesVista.svelte';
  import { RecorridosAjenos } from '$lib/recorridosAjenos';
  import { destinoDeLaFlecha, enterEsDelTablero } from '$lib/moverConFlecha';
  import {
    alRecibirAviso, enviarAviso, avisoPing, avisoRecorrido, avisoRecorridoBorrado, avisoArrastre, avisoArrastreFin
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
  let canvasFichas;
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
  // La ficha desde la que se dibujó la flecha (la pulsada con el botón derecho): Enter la pone en la casilla final.
  // Se ve marcada mientras dure la flecha y se pueda mover.
  /** @type {number | null} */
  let origenRecorrido = $state(null);
  // Las flechas de los demás (una por persona, cada una de su color), recibidas por aviso. `versionAjenos` avisa al
  // pintado de que cambiaron.
  const recorridosAjenos = new RecorridosAjenos();
  let versionAjenos = $state(0);

  // Pings en curso (clic central). No es reactivo: los anima el bucle de pintado mientras duren.
  /** @type {import("$lib/marcasTablero").Ping[]} */
  const pings = [];
  // Pulsación del botón central: si se suelta casi sin moverse es un ping; si no, era mover la vista.
  /** @type {{ x: number, y: number } | null} */
  let pulsacionCentral = null;
  const MOVIMIENTO_MAX_CLIC_PX = 5;

  // Clic derecho que empezó sobre una ficha: si se suelta sin salir de su casilla ni mover el ratón,
  // la ficha gira 90° a la derecha (con Ctrl pulsado, `espejo`, se pone en espejo). La flecha de recorrido
  // (y su casilla amarilla) solo aparece cuando el cursor sale de la casilla de salida.
  /** @type {{ x: number, y: number, idInstancia: number, salida: import("$lib/partida.svelte").Casilla, espejo: boolean } | null} */
  let derechoSobreFicha = null;
  // Pulsación izquierda en el fondo: si se suelta sin arrastrar, las fichas pierden el foco.
  /** @type {{ x: number, y: number } | null} */
  let pulsacionIzquierda = null;
  // Ctrl + arrastrar en el fondo: recuadro de selección (px del mapa) y dónde se pulsó (px de la ventana).
  // Al soltar, las fichas que toca se añaden a las que tienen el foco.
  /** @type {{ desde: { x: number, y: number }, hasta: { x: number, y: number }, inicio: { x: number, y: number } } | null} */
  let recuadro = $state.raw(null);

  // Instancias colocadas en el mapa abierto (en orden de dibujo: la última, encima)
  const fichas = $derived(fichasDelMapa(partida.instanciasOrdenadas, typeof mapa?.id === 'number' ? mapa.id : null));

  /** @param {import("$lib/partida.svelte").Instancia} i */
  const casillaDe = (i) => (typeof i.coordenadas === 'object' ? i.coordenadas.Casilla : null);

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
    finArrastreEnVivo();
    arrastreAjeno.limpiar();
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
  //  * izquierdo: sobre una ficha, clic = foco (con Ctrl, añadirla o quitarla) y arrastrar = moverla
  //    (si tiene el foco, junto con las demás que lo tienen); en el fondo, arrastrar mueve la vista
  //    (con Ctrl, recuadro que añade al foco) y un clic quita el foco.
  //  * central: clic = ping; arrastrar también mueve la vista.
  //  * derecho: arrastrar mide un recorrido por casillas; sobre una ficha sin arrastrar, la gira.
  /** @param {PointerEvent} e */
  function alPresionar(e) {
    const p = puntoDelMapa(e);
    const casillaPulsada = casillaEn(casillas, p.x, p.y);
    // Por el rectángulo de la ficha y no por la casilla: una que quedó fuera del mapa (tras agrandar la
    // rejilla, hasta que Rust la reencaja) también se puede coger
    const ficha = fichaEnPunto(fichas, casillas, p.x, p.y);
    const ctrl = e.ctrlKey || e.metaKey; // Ctrl: selección múltiple, igual que en el banco

    if (e.button === 0 && ficha) {
      e.preventDefault();
      // Punto por el que se coge, para que la ficha no salte bajo el cursor al arrastrarla
      const r = rectanguloFicha(cam, casillas, { instancia: ficha, casilla: casillaDe(ficha) ?? { x: 0, y: 0 } });
      const caja = contenedor.getBoundingClientRect();
      const agarre = { x: (e.clientX - caja.left - r.x) / r.w, y: (e.clientY - caja.top - r.y) / r.h };
      // Si tiene el foco, se arrastra con todas las de este mapa que lo tienen
      const grupo = partida.estaSeleccionada(ficha.id_instancia)
        ? fichas.filter((f) => partida.estaSeleccionada(f.instancia.id_instancia)).map((f) => f.instancia)
        : [];
      arrastre.preparar(
        e,
        { instancia: ficha, desde: 'tablero', grupo: grupo.length > 1 ? grupo : undefined },
        () => partida.seleccionarInstancia(ficha, ctrl),
        agarre
      );
      return;
    }

    if (e.button === 0 && ctrl) {
      e.preventDefault();
      recuadro = { desde: p, hasta: p, inicio: { x: e.clientX, y: e.clientY } };
      contenedor.setPointerCapture(e.pointerId);
      return;
    }

    if (e.button === 2) {
      e.preventDefault();
      const salida = casillaPulsada;
      if (!salida) return; // se empieza a medir solo desde dentro del mapa
      midiendo = true;
      if (ficha) {
        // Sobre una ficha aún no se marca nada: puede ser un giro
        derechoSobreFicha = { x: e.clientX, y: e.clientY, idInstancia: ficha.id_instancia, salida, espejo: ctrl };
      } else {
        derechoSobreFicha = null;
        origenRecorrido = null;
        cambiarRecorrido([salida]);
      }
      contenedor.setPointerCapture(e.pointerId);
      return;
    }
    if (e.button !== 0 && e.button !== 1) return;
    e.preventDefault();
    panActivo = true;
    ultimo = { x: e.clientX, y: e.clientY };
    pulsacionCentral = e.button === 1 ? { x: e.clientX, y: e.clientY } : null;
    pulsacionIzquierda = e.button === 0 ? { x: e.clientX, y: e.clientY } : null;
    contenedor.setPointerCapture(e.pointerId);
  }

  /** @param {PointerEvent} e */
  function alMover(e) {
    if (recuadro) {
      recuadro = { ...recuadro, hasta: puntoDelMapa(e) };
      return;
    }
    if (midiendo) {
      const p = puntoDelMapa(e);
      const aqui = casillaMasCercana(casillas, p.x, p.y);
      if (recorrido.length === 0) {
        // Clic derecho sobre una ficha: la flecha empieza al salir de su casilla
        const d = derechoSobreFicha;
        if (!d || (aqui.x === d.salida.x && aqui.y === d.salida.y)) return;
        origenRecorrido = d.idInstancia; // la flecha sale de una ficha: Enter la moverá hasta el final
        cambiarRecorrido(extenderRecorrido([d.salida], aqui));
        return;
      }
      const siguiente = extenderRecorrido(recorrido, aqui);
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
    if (
      pulsacionIzquierda &&
      Math.hypot(e.clientX - pulsacionIzquierda.x, e.clientY - pulsacionIzquierda.y) > MOVIMIENTO_MAX_CLIC_PX
    ) {
      pulsacionIzquierda = null;
    }
    cam = { ...cam, x: cam.x + e.clientX - ultimo.x, y: cam.y + e.clientY - ultimo.y };
    ultimo = { x: e.clientX, y: e.clientY };
  }

  /** @param {PointerEvent} e */
  function alSoltar(e) {
    if (recuadro && e.button === 0) {
      const r = recuadro;
      recuadro = null;
      // Un Ctrl+clic sin arrastrar no hace nada (ni quita el foco)
      if (Math.hypot(e.clientX - r.inicio.x, e.clientY - r.inicio.y) > MOVIMIENTO_MAX_CLIC_PX) {
        const ids = fichasEnRectangulo(fichas, casillas, r.desde, puntoDelMapa(e)).map((i) => i.id_instancia);
        partida.seleccionarInstancias(ids, true);
      }
    }
    if (midiendo && e.button === 2) {
      midiendo = false; // el recorrido se queda dibujado hasta el siguiente clic
      enviarRecorridoPendiente(); // y los demás reciben ya su estado definitivo
      // Clic derecho sin arrastrar sobre una ficha: gira 90° a la derecha; con Ctrl, la pone en espejo
      const d = derechoSobreFicha;
      if (d && recorrido.length === 0 && Math.hypot(e.clientX - d.x, e.clientY - d.y) <= MOVIMIENTO_MAX_CLIC_PX) {
        const instancia = partida.instancias.find((i) => i.id_instancia === d.idInstancia);
        // El jugador solo gira y espeja las suyas (el host lo vuelve a comprobar)
        if (instancia && partida.controla(instancia)) {
          void (d.espejo ? partida.espejarInstancia(d.idInstancia) : partida.rotarInstancia(d.idInstancia));
        }
      }
      derechoSobreFicha = null;
    }
    if (pulsacionIzquierda && e.button === 0) partida.seleccionarInstancia(null);
    pulsacionIzquierda = null;
    if (pulsacionCentral && e.button === 1) {
      const p = puntoDelMapa(e);
      const casilla = casillaEn(casillas, p.x, p.y);
      if (casilla) {
        lanzarPing(casilla, ajustesVista.colorFlecha);
        if (idMapa() !== null) enviarAviso(avisoPing(idMapa(), casilla, ajustesVista.colorFlecha));
      }
    }
    pulsacionCentral = null;
    panActivo = false;
    if (contenedor?.hasPointerCapture?.(e.pointerId)) contenedor.releasePointerCapture(e.pointerId);
  }

  // Un pointercancel (p. ej. la ventana pierde el foco) no debe dejar un ping ni una medida a medias
  /** @param {PointerEvent} e */
  function alCancelar(e) {
    recuadro = null;
    midiendo = false;
    derechoSobreFicha = null;
    pulsacionIzquierda = null;
    pulsacionCentral = null;
    panActivo = false;
    if (contenedor?.hasPointerCapture?.(e.pointerId)) contenedor.releasePointerCapture(e.pointerId);
  }

  // Id del mapa abierto (null si no hay uno guardado aún): los avisos van ligados a un mapa
  function idMapa() {
    return typeof mapa?.id === 'number' ? mapa.id : null;
  }

  // El recorrido propio se manda como mucho cada MIN_ENTRE_RECORRIDOS_MS, pero SIEMPRE se acaba mandando el último
  // estado (al terminar el intervalo y, sin esperar, al soltar el botón): si el último se perdiera o se retrasara, los
  // demás se quedarían con una flecha vieja. Los pasos intermedios que se saltan no importan: cada aviso es la flecha entera.
  const MIN_ENTRE_RECORRIDOS_MS = 40;
  /** @type {import("$lib/avisosTablero").AvisoTablero | null} */
  let recorridoPendiente = null;
  /** @type {ReturnType<typeof setTimeout> | null} */
  let relojRecorrido = null;
  let ultimoEnvioRecorrido = 0;

  // Manda ya lo último que quedaba por mandar (si hay algo)
  function enviarRecorridoPendiente() {
    if (relojRecorrido) clearTimeout(relojRecorrido);
    relojRecorrido = null;
    const aviso = recorridoPendiente;
    recorridoPendiente = null;
    if (!aviso) return;
    ultimoEnvioRecorrido = performance.now();
    enviarAviso(aviso);
  }

  // Descarta lo pendiente sin mandarlo (al cambiar de mapa: ya no tiene sentido)
  function cancelarRecorridoPendiente() {
    if (relojRecorrido) clearTimeout(relojRecorrido);
    relojRecorrido = null;
    recorridoPendiente = null;
  }

  // Cambia el recorrido propio y avisa. `[]` = quitarlo (eso sí se manda enseguida).
  /** @param {import("$lib/partida.svelte").Casilla[]} casillas */
  function cambiarRecorrido(casillas) {
    recorrido = casillas;
    if (!casillas.length) origenRecorrido = null; // sin flecha, no hay ficha marcada
    const id = idMapa();
    if (id === null) return;
    recorridoPendiente = casillas.length ? avisoRecorrido(id, casillas, ajustesVista.colorFlecha) : avisoRecorridoBorrado(id);
    if (!casillas.length) return enviarRecorridoPendiente();
    if (relojRecorrido) return; // ya hay un envío programado: llevará el último estado
    const espera = Math.max(0, MIN_ENTRE_RECORRIDOS_MS - (performance.now() - ultimoEnvioRecorrido));
    relojRecorrido = setTimeout(enviarRecorridoPendiente, espera);
  }

  // Enter con la flecha dibujada desde una ficha: la pone en la casilla final (como arrastrarla hasta allí). Los mismos
  // permisos que al moverla a mano: la pantalla comprueba `partida.controla` y el host vuelve a comprobarlo.
  /** @param {KeyboardEvent} e */
  function alPulsarEnter(e) {
    if (origenRecorrido === null || !enterEsDelTablero(e)) return;
    const id = origenRecorrido;
    const destino = destinoDeLaFlecha({
      recorrido,
      instancia: partida.instancias.find((i) => i.id_instancia === id),
      idMapa: idMapa(),
      controla: (i) => partida.controla(i)
    });
    if (!destino) return;
    e.preventDefault();
    midiendo = false; // por si aún se tiene el botón pulsado
    derechoSobreFicha = null;
    cambiarRecorrido([]); // la flecha se quita (y los demás lo saben)
    void partida.moverInstancia(id, destino);
  }

  // Un aviso de otro jugador (llega por el evento "aviso-tablero"; ver $lib/avisosTablero)
  /** @param {import("$lib/avisosTablero").AvisoTablero} aviso */
  function alLlegarAviso(aviso) {
    if ('Ping' in aviso) {
      if (aviso.Ping.id_mapa === idMapa()) lanzarPing(aviso.Ping.casilla, aviso.Ping.color);
    } else if ('Recorrido' in aviso || 'RecorridoBorrado' in aviso) {
      if (recorridosAjenos.recibir(aviso, idMapa())) versionAjenos++;
    } else if ('Arrastre' in aviso || 'ArrastreFin' in aviso) {
      arrastreAjeno.recibir(aviso); // solo se dibuja la de este mapa (`arrastreAjeno.de`)
    }
    // `Foco` lo atiende +page.svelte (lo ven también «Fichas activas» y la lista de parámetros)
  }

  /**
   * @param {import("$lib/partida.svelte").Casilla} casilla
   * @param {import("$lib/colorMarca").ColorMarca} [color] el de quien lo hizo
   */
  function lanzarPing(casilla, color) {
    pings.push({ casilla, inicio: performance.now(), color });
    marcasPendientes = true;
    programar();
  }

  // Cualquier clic, en el tablero o fuera de él, borra el recorrido que quedó dibujado.
  // Va en fase de captura: si el clic es otro botón derecho, se borra antes de empezar el nuevo.
  function alPulsarEnLaVentana() {
    if (!midiendo && recorrido.length) cambiarRecorrido([]);
  }

  // ---------------------------------------------------------------------------
  // Arrastre en vivo: los demás ven por dónde pasa lo que arrastras. Es un aviso efímero (como el recorrido): la
  // posición real se guarda solo al soltar. Se envía cuando cambia alguna casilla (con un mínimo entre envíos y un
  // latido mientras dura, para que los demás no den la ficha por abandonada).
  const MIN_ENTRE_ENVIOS_MS = 50;
  const LATIDO_MS = 3000;
  /** @type {{ clave: string, piezas: { id_instancia: number, casilla: any }[], mapa: number } | null} */
  let arrastreEnviado = null;
  /** @type {{ clave: string, piezas: { id_instancia: number, casilla: any }[] } | null} */
  let arrastrePendiente = null;
  /** @type {ReturnType<typeof setTimeout> | null} */
  let temporizadorArrastre = null;
  /** @type {ReturnType<typeof setInterval> | null} */
  let latidoArrastre = null;
  let ultimoEnvioArrastre = 0;

  // Las casillas por las que pasan ahora las fichas que se arrastran (null si no se arrastra nada sobre este mapa)
  function piezasArrastradas() {
    const a = arrastre.actual;
    if (!a || a.zona !== 'tablero' || !('instancia' in a.origen)) return null;
    const destino = arrastre.casillaActual();
    if (!destino || destino.id_mapa !== idMapa()) return null;
    const grupo = instanciasArrastradas(a.origen);
    if (grupo.length <= 1) return [{ id_instancia: a.origen.instancia.id_instancia, casilla: destino.casilla }];
    // Un grupo: todas se desplazan lo mismo que la cogida (lo mismo que dibuja el amarillo y hará Rust)
    const casillaCogida = casillaDe(a.origen.instancia);
    if (!casillaCogida) return null;
    const items = grupo.flatMap((i) => (casillaDe(i) ? [{ instancia: i, casilla: /** @type {any} */ (casillaDe(i)) }] : []));
    const destinos = desplazarGrupo(
      casillas,
      items.map((f) => ({ casilla: f.casilla, lados: ladosInstancia(f.instancia) })),
      destino.casilla.x - casillaCogida.x,
      destino.casilla.y - casillaCogida.y
    );
    return items.map((f, k) => ({ id_instancia: f.instancia.id_instancia, casilla: destinos[k] }));
  }

  function actualizarArrastreEnVivo() {
    const piezas = piezasArrastradas();
    if (!piezas) return finArrastreEnVivo();
    const clave = JSON.stringify(piezas.map((p) => [p.id_instancia, p.casilla.x, p.casilla.y]));
    if (arrastreEnviado?.clave === clave) {
      arrastrePendiente = null;
      return;
    }
    arrastrePendiente = { clave, piezas };
    if (temporizadorArrastre) return;
    const espera = Math.max(0, MIN_ENTRE_ENVIOS_MS - (performance.now() - ultimoEnvioArrastre));
    temporizadorArrastre = setTimeout(() => {
      temporizadorArrastre = null;
      const p = arrastrePendiente;
      arrastrePendiente = null;
      if (p) enviarArrastre(p);
    }, espera);
  }

  /** @param {{ clave: string, piezas: { id_instancia: number, casilla: any }[] }} p */
  function enviarArrastre(p) {
    const mapaId = idMapa();
    if (mapaId === null) return;
    arrastreEnviado = { clave: p.clave, piezas: p.piezas, mapa: mapaId };
    ultimoEnvioArrastre = performance.now();
    enviarAviso(avisoArrastre(mapaId, p.piezas));
    latidoArrastre ??= setInterval(() => {
      if (arrastreEnviado) enviarAviso(avisoArrastre(arrastreEnviado.mapa, arrastreEnviado.piezas));
    }, LATIDO_MS);
  }

  // Se soltó, se canceló o se salió del tablero: los demás dejan de ver el arrastre
  function finArrastreEnVivo() {
    if (temporizadorArrastre) clearTimeout(temporizadorArrastre);
    if (latidoArrastre) clearInterval(latidoArrastre);
    temporizadorArrastre = latidoArrastre = null;
    arrastrePendiente = null;
    if (arrastreEnviado) {
      enviarAviso(avisoArrastreFin(arrastreEnviado.mapa, arrastreEnviado.piezas.map((p) => p.id_instancia)));
      arrastreEnviado = null;
    }
  }

  $effect(() => {
    void [arrastre.actual, cam, casillas, ancho, alto];
    untrack(actualizarArrastreEnVivo);
  });

  // Cuando la posición de verdad de lo que otro soltó ya está aquí, su «fantasma» sobra
  $effect(() => {
    const posiciones = new Map(fichas.map((f) => [f.instancia.id_instancia, f.casilla]));
    untrack(() => arrastreAjeno.alLlegarPosiciones(posiciones));
  });

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
      cancelarRecorridoPendiente(); // lo pendiente era del mapa anterior
      if (recorrido.length && mapaDeLasMarcas !== null) enviarAviso(avisoRecorridoBorrado(mapaDeLasMarcas));
      mapaDeLasMarcas = id;
      arrastreAjeno.limpiar();
      recorrido = [];
      origenRecorrido = null;
      recorridosAjenos.limpiar();
      versionAjenos++;
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

  // Capa de fichas: depende de las instancias del mapa, la selección, la cámara y el arrastre en curso.
  let fichasPendientes = true;
  $effect(() => {
    void [fichas, origenRecorrido, partida.idsSeleccionados, partida.idsFocoAjeno, partida.definiciones, cam, ancho, alto, dpr, casillas, arrastre.actual, arrastreAjeno.piezas];
    void Object.values(partida.valoresPendientes); // la barra de vida y los radios siguen a la rueda
    // Las miniaturas se piden aquí y se leen para que, al llegar, se repinten las fichas
    const a = arrastre.actual;
    const bases = fichas.map((f) => f.instancia.base);
    if (a) bases.push(baseDe(a.origen));
    for (const b of bases) {
      miniaturas.asegurarFicha(b);
      void miniaturas.deFicha(b);
    }
    fichasPendientes = true;
    programar();
  });

  function pintarFichas() {
    const ctx = prepararCanvas(canvasFichas, ancho, alto, dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0); // en px de pantalla
    const repintar = () => {
      fichasPendientes = true;
      programar();
    };
    // La foto de la ficha es su miniatura (la misma que usa todo lo demás); null hasta que esté lista
    /** @param {import("$lib/partida.svelte").Entidad} base */
    const imagen = (base) => {
      const url = base.foto_url ? miniaturas.deFicha(base) : '';
      return url ? imagenDe(url, repintar) : null;
    };

    // 1. Las formas de los parámetros de las fichas con foco (debajo de todas las fichas). Se quedan
    //    donde estaba la ficha aunque se esté arrastrando: así se ve hasta dónde puede llegar.
    for (const f of fichas) {
      if (!partida.estaSeleccionada(f.instancia.id_instancia)) continue;
      const base = f.instancia.base;
      dibujarFormas(
        ctx, cam, casillas, f, partida.definiciones.filter((d) => veParametro(f.instancia, d.id)),
        (d) => (d.tiene_maximo ? partida.valorMostrado(f.instancia, d.id) : numeroDe(base, d.id)),
        (d) => numeroDe(base, d.id)
      );
    }

    // 2. Las fichas (la que se arrastra no se dibuja en su sitio: va bajo el cursor)
    for (const f of fichas) {
      if (arrastre.arrastrando(f.instancia.id_instancia)) continue;
      // Si otro la está arrastrando, se ve por donde pasa (su posición real no cambia hasta que la suelte)
      const porDondePasa = arrastreAjeno.de(f.instancia.id_instancia, idMapa());
      const enTransito = !!porDondePasa && (porDondePasa.x !== f.casilla.x || porDondePasa.y !== f.casilla.y);
      dibujarFicha(ctx, cam, casillas, enTransito ? { instancia: f.instancia, casilla: /** @type {any} */ (porDondePasa) } : f, imagen(f.instancia.base), {
        seleccionada: partida.estaSeleccionada(f.instancia.id_instancia),
        marcadaParaMover: origenRecorrido === f.instancia.id_instancia && partida.controla(f.instancia),
        focoAjeno: partida.idsFocoAjeno.includes(f.instancia.id_instancia),
        apagada: partida.estaCaida(f.instancia),
        fantasma: enTransito
      });
    }

    // 3. Lo que se arrastra sobre el tablero: dónde caerá (amarillo) y la ficha "cogida" bajo el cursor
    const a = arrastre.actual;
    if (!a || a.zona !== 'tablero') return;
    const destino = arrastre.casillaActual();
    const instancia = 'instancia' in a.origen ? a.origen.instancia : fichaDeEntidad(baseDe(a.origen));
    // Las demás del grupo (si se arrastran varias del tablero), con su casilla de ahora
    const otras = instanciasArrastradas(a.origen)
      .filter((i) => i.id_instancia !== instancia.id_instancia)
      .flatMap((i) => (casillaDe(i) ? [{ instancia: i, casilla: /** @type {any} */ (casillaDe(i)) }] : []));
    const casillaCogida = casillaDe(instancia);

    if (destino && destino.id_mapa === idMapa()) {
      if ('entidades' in a.origen) {
        // Varias fichas del banco: dónde caerá cada una (la misma cuadrícula que hará Rust)
        const lados = a.origen.entidades.map(ladosDeEntidad);
        disposicionEnCuadricula(casillas, destino.casilla, lados).forEach((c, k) => dibujarDestino(ctx, cam, casillas, c, lados[k]));
      } else if (otras.length > 0 && casillaCogida) {
        // Un grupo del tablero: todas se desplazan lo mismo que la cogida (lo mismo que hará Rust)
        const grupo = [{ instancia, casilla: casillaCogida }, ...otras].map((f) => ({ casilla: f.casilla, lados: ladosInstancia(f.instancia) }));
        const dx = destino.casilla.x - casillaCogida.x;
        const dy = destino.casilla.y - casillaCogida.y;
        desplazarGrupo(casillas, grupo, dx, dy).forEach((c, k) => dibujarDestino(ctx, cam, casillas, c, grupo[k].lados));
      } else {
        dibujarDestino(ctx, cam, casillas, destino.casilla, a.lados);
      }
    }

    const caja = contenedor.getBoundingClientRect();
    const w = a.lados[0] * casillas.anchoCasilla * cam.zoom;
    const h = a.lados[1] * casillas.altoCasilla * cam.zoom;
    const agarre = a.agarre ?? { x: 0.5, y: 0.5 };
    const en = { x: a.x - caja.left - agarre.x * w, y: a.y - caja.top - agarre.y * h };
    // Las demás del grupo van "cogidas" con ella, a la misma distancia que tienen en el tablero
    if (casillaCogida) {
      const origen = rectanguloFicha(cam, casillas, { instancia, casilla: casillaCogida });
      for (const f of otras) {
        const r = rectanguloFicha(cam, casillas, f);
        dibujarFicha(ctx, cam, casillas, f, imagen(f.instancia.base), { en: { x: en.x + r.x - origen.x, y: en.y + r.y - origen.y }, sombra: true });
      }
    }
    dibujarFicha(ctx, cam, casillas, { instancia, casilla: { x: 0, y: 0 } }, imagen(instancia.base), { en, sombra: true });
  }

  // Una ficha del banco aún sin instancia, para poder dibujarla igual que una instancia
  /** @param {import("$lib/partida.svelte").Entidad} entidad */
  function fichaDeEntidad(entidad) {
    return /** @type {import("$lib/partida.svelte").Instancia} */ ({
      id_instancia: -1, id_mapa: null, base: entidad, valores_actuales: [], coordenadas: 'NoPuesto', rotacion: 0, orden: 0
    });
  }

  // Depende de la cámara, del tamaño del tablero, de las casillas y del recorrido.
  $effect(() => {
    void [cam, ancho, alto, dpr, casillas, recorrido, versionAjenos, ajustesVista.colorFlecha, recuadro];
    marcasPendientes = true;
    programar();
  });

  function pintarMarcas() {
    const ctx = prepararCanvas(canvasMarcas, ancho, alto, dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0); // en px de pantalla
    for (const f of recorridosAjenos.lista()) dibujarRecorrido(ctx, cam, casillas, f.casillas, f.color);
    dibujarRecorrido(ctx, cam, casillas, recorrido, ajustesVista.colorFlecha);
    if (recuadro) dibujarRecuadro(ctx, recuadro.desde, recuadro.hasta);

    const ahora = performance.now();
    for (let i = pings.length - 1; i >= 0; i--) {
      if (!pingVivo(pings[i], ahora)) pings.splice(i, 1);
    }
    for (const ping of pings) dibujarPing(ctx, cam, casillas, ping, ahora);
  }

  /**
   * El recuadro de selección (Ctrl + arrastrar), con sus esquinas en px del mapa
   * @param {CanvasRenderingContext2D} ctx
   * @param {{ x: number, y: number }} desde
   * @param {{ x: number, y: number }} hasta
   */
  function dibujarRecuadro(ctx, desde, hasta) {
    const a = mundoAPantalla(cam, desde.x, desde.y);
    const b = mundoAPantalla(cam, hasta.x, hasta.y);
    const x = Math.min(a.x, b.x);
    const y = Math.min(a.y, b.y);
    const w = Math.abs(a.x - b.x);
    const h = Math.abs(a.y - b.y);
    ctx.save();
    ctx.fillStyle = 'rgba(250, 250, 250, 0.08)';
    ctx.fillRect(x, y, w, h);
    ctx.strokeStyle = 'rgba(250, 250, 250, 0.85)';
    ctx.lineWidth = 1;
    ctx.setLineDash([5, 4]);
    ctx.strokeRect(Math.round(x) + 0.5, Math.round(y) + 0.5, Math.round(w), Math.round(h));
    ctx.restore();
  }

  function pintar() {
    rafId = 0;
    if (!canvasFondo || !canvasRejilla || !canvasFichas || !canvasMarcas) return;

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

    if (fichasPendientes && ancho && alto) {
      fichasPendientes = false;
      pintarFichas();
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
    // Cómo pasa el cursor a una casilla del mapa abierto, para soltar fichas (ver $lib/arrastreFicha)
    // Con `agarre` (el punto por el que se cogió la ficha), lo que cuenta es dónde queda el centro
    // de la ficha, no el cursor: así cae donde se ve.
    const dejarDeResolver = arrastre.registrarTablero((clientX, clientY, lados, agarre) => {
      const id = idMapa();
      if (id === null) return null;
      const r = contenedor.getBoundingClientRect();
      const p = pantallaAMundo(cam, clientX - r.left, clientY - r.top);
      if (p.x < 0 || p.y < 0 || p.x >= casillas.ancho || p.y >= casillas.alto) return null;
      const w = lados[0] * casillas.anchoCasilla;
      const h = lados[1] * casillas.altoCasilla;
      const centro = agarre ? { x: p.x + (0.5 - agarre.x) * w, y: p.y + (0.5 - agarre.y) * h } : p;
      return { id_mapa: id, casilla: origenAlSoltar(casillas, centro.x, centro.y, lados) };
    });

    // Qué ficha hay bajo el cursor, para soltar encima a un jugador (ver $lib/arrastreJugador)
    const dejarDeResolverJugador = arrastreJugador.registrarTablero((clientX, clientY) => {
      const r = contenedor.getBoundingClientRect();
      const p = pantallaAMundo(cam, clientX - r.left, clientY - r.top);
      return fichaEnPunto(fichas, casillas, p.x, p.y)?.base.id ?? null;
    });

    return () => {
      observador.disconnect();
      window.removeEventListener('pointerdown', alPulsarEnLaVentana, true);
      dejarDeEscuchar();
      dejarDeResolver();
      dejarDeResolverJugador();
      cancelAnimationFrame(rafId);
    };
  });
</script>

<svelte:window onkeydown={alPulsarEnter} />

<div
  class="tablero"
  class:arrastrando={panActivo}
  bind:this={contenedor}
  data-destino="tablero"
  data-conserva-foco
  role="application"
  aria-label="Tablero de juego"
  title={ayuda("Rueda: zoom\nArrastrar: mover la vista o una ficha (las que tienen foco, juntas)\nArrastrar click derecho: Dibujar una flecha\nEnter cuando el origen de una flecha está sobre una instancia: mover la instancia\nCtrl+clic: añadir o quitar del foco\nCtrl+arrastrar en el fondo: seleccionar con un recuadro\nDoble clic: centrar\nClic central: aviso\nArrastrar con botón derecho: medir\nClic derecho: rotar\nCtrl+clic derecho: rotar y espejar")}
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
  <canvas bind:this={canvasFichas}></canvas>
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