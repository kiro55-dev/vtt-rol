<!-- src/routes/componentes/VentanaFicha.svelte -->
<script lang="ts">
  import { onMount, tick } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { partida, type Entidad } from "$lib/partida.svelte";
  import { importarImagen, urlDeImagen } from "$lib/imagenes";

  interface ArchivoAdjunto {
    ruta: string;
    entidad_archivo: Entidad;
  }

  let { entidad }: { entidad: Entidad } = $props();

  const EXTENSIONES_IMAGEN = ["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif", "svg"];
  const ES_IMAGEN = new RegExp(`\\.(${EXTENSIONES_IMAGEN.join("|")})$`, "i");

  let pos = $state({ x: 260, y: 70 });
  let pestanaActiva = $state<number>(0);

  // Textos cargados en memoria
  let textosArchivos = $state<Record<string, string>>({});
  let textoActual = $state("");

  // Control de edición de nombre de archivo con doble clic
  let editandoTabIndice = $state<number | null>(null);
  let textoEdicionTab = $state("");

  // Control de scroll y flechas
  let barraTabs = $state<HTMLDivElement | null>(null);
  let puedeScrollIzq = $state(false);
  let puedeScrollDer = $state(false);

  function nombreLimpio(ruta: string | null | undefined): string {
    if (!ruta) return "Documento_1.txt";
    return ruta.split(/[/\\]/).pop() ?? ruta;
  }

  function esTexto(ruta: string | null | undefined): boolean {
    if (!ruta) return false;
    return ruta.toLowerCase().endsWith(".txt");
  }

  function comprobarScroll() {
    if (!barraTabs) return;
    puedeScrollIzq = barraTabs.scrollLeft > 2;
    puedeScrollDer = barraTabs.scrollLeft + barraTabs.clientWidth < barraTabs.scrollWidth - 2;
  }

  function scrollIzquierda() {
    barraTabs?.scrollBy({ left: -100, behavior: "smooth" });
  }

  function scrollDerecha() {
    barraTabs?.scrollBy({ left: 100, behavior: "smooth" });
  }

  async function cargarContenidoPestana(indice: number) {
    if (entidad.archivos_adjuntos.length === 0) {
      textoActual = "";
      return;
    }
    pestanaActiva = indice;
    const ruta = entidad.archivos_adjuntos[indice];
    if (!ruta || !esTexto(ruta)) {
      textoActual = "";
      return;
    }

    if (textosArchivos[ruta] !== undefined) {
      textoActual = textosArchivos[ruta];
    } else {
      try {
        const contenido = await invoke<string>("leer_archivo_texto", { rutaRelativa: ruta });
        textosArchivos[ruta] = contenido;
        textoActual = contenido;
      } catch (err) {
        console.error("Error al leer texto:", err);
        textoActual = "";
      }
    }
  }

  function alModificarTexto(e: Event) {
    const val = (e.target as HTMLTextAreaElement).value;
    textoActual = val;
    const ruta = entidad.archivos_adjuntos[pestanaActiva];
    if (ruta) {
      textosArchivos[ruta] = val;
    }
  }

  // Creación desde Rust con nombre limpio secuencial
  async function crearNuevoTxt() {
    try {
      const nuevoAdjunto = await invoke<ArchivoAdjunto>("crear_documento_texto", {
        entidad
      });

      entidad.archivos_adjuntos.push(nuevoAdjunto.ruta);
      textosArchivos[nuevoAdjunto.ruta] = "";
      await tick();
      cargarContenidoPestana(entidad.archivos_adjuntos.length - 1);
      comprobarScroll();
    } catch (err) {
      console.error("Error al crear documento en Rust:", err);
    }
  }

  // Renombrar con doble clic
  function iniciarRenombrado(indice: number, ruta: string) {
    editandoTabIndice = indice;
    textoEdicionTab = nombreLimpio(ruta);
  }

  async function confirmarRenombrado(indice: number) {
    if (editandoTabIndice === null) return;
    editandoTabIndice = null;

    const rutaVieja = entidad.archivos_adjuntos[indice];
    if (!rutaVieja) return;

    const nuevoNombre = textoEdicionTab.trim();
    if (!nuevoNombre || nuevoNombre === nombreLimpio(rutaVieja)) return;

    try {
      const adjuntoActualizado = await invoke<ArchivoAdjunto>("renombrar_archivo_adjunto", {
        adjunto: { ruta: rutaVieja, entidad_archivo: entidad },
        nuevoNombre
      });

      entidad.archivos_adjuntos[indice] = adjuntoActualizado.ruta;

      // Transferir contenido en memoria a la nueva ruta
      if (textosArchivos[rutaVieja] !== undefined) {
        textosArchivos[adjuntoActualizado.ruta] = textosArchivos[rutaVieja];
        delete textosArchivos[rutaVieja];
      }
    } catch (err) {
      console.error("Error al renombrar archivo en Rust:", err);
    }
  }

  function desasociarAdjunto(indice: number) {
    const [eliminado] = entidad.archivos_adjuntos.splice(indice, 1);
    if (eliminado) delete textosArchivos[eliminado];
    pestanaActiva = Math.max(0, pestanaActiva - 1);
    cargarContenidoPestana(pestanaActiva);
    comprobarScroll();
  }

  async function guardarTodo() {
    for (const [ruta, contenido] of Object.entries(textosArchivos)) {
      try {
        await invoke("guardar_archivo_texto", {
          adjunto: { ruta, entidad_archivo: entidad },
          contenido
        });
      } catch (err) {
        console.error("Error al guardar texto:", err);
      }
    }
    await partida.guardarEntidad(entidad);
  }

  function alTeclearVentana(e: KeyboardEvent) {
    if (e.key === "Enter") {
      if ((e.target as HTMLElement)?.tagName === "TEXTAREA" || editandoTabIndice !== null) return;
      e.preventDefault();
      guardarTodo();
    } else if (e.key === "Escape") {
      partida.cerrarEditorEntidad();
    }
  }

  function iniciarArrastre(e: PointerEvent) {
    if (e.button !== 0) return;
    const inicioX = e.clientX;
    const inicioY = e.clientY;
    const origenX = pos.x;
    const origenY = pos.y;

    const alMover = (ev: PointerEvent) => {
      pos.x = Math.max(10, Math.min(window.innerWidth - 460, origenX + (ev.clientX - inicioX)));
      pos.y = Math.max(10, Math.min(window.innerHeight - 300, origenY + (ev.clientY - inicioY)));
    };
    const alSoltar = () => {
      window.removeEventListener("pointermove", alMover);
      window.removeEventListener("pointerup", alSoltar);
    };
    window.addEventListener("pointermove", alMover);
    window.addEventListener("pointerup", alSoltar);
  }

  async function seleccionarFoto(e: MouseEvent) {
    e.preventDefault();
    const ruta = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Imágenes", extensions: EXTENSIONES_IMAGEN }]
    });
    if (typeof ruta === "string") {
      entidad.foto_url = await importarImagen(ruta);
    }
  }

  onMount(() => {
    if (entidad.archivos_adjuntos.length > 0) {
      cargarContenidoPestana(0);
    }

    const resizeObserver = new ResizeObserver(() => comprobarScroll());
    if (barraTabs) resizeObserver.observe(barraTabs);

    let unlisten: (() => void) | null = null;
    let desmontado = false;

    try {
      getCurrentWebview()
        .onDragDropEvent(async (event) => {
          if (event.payload.type !== "drop") return;
          const rutas = event.payload.paths ?? [];
          if (rutas.length === 0) return;

          const escala = window.devicePixelRatio || 1;
          const { x, y } = event.payload.position;
          const el = document.elementFromPoint(x / escala, y / escala);

          // 1. Arrastre a foto
          if (el?.closest(".caja-foto")) {
            const rutaImg = rutas.find((r) => ES_IMAGEN.test(r));
            if (rutaImg) {
              entidad.foto_url = await importarImagen(rutaImg);
            }
            return;
          }

          // 2. Arrastre a archivos
          if (el?.closest(".zona-archivos")) {
            for (const r of rutas) {
              try {
                const adjunto = await invoke<ArchivoAdjunto>("importar_archivo", {
                  adjunto: { ruta: "", entidad_archivo: entidad },
                  origen: r
                });
                entidad.archivos_adjuntos.push(adjunto.ruta);
              } catch (err) {
                console.error("Error al importar adjunto:", err);
              }
            }
            await tick();
            cargarContenidoPestana(entidad.archivos_adjuntos.length - 1);
            comprobarScroll();
          }
        })
        .then((fn) => (desmontado ? fn() : (unlisten = fn)))
        .catch((err) => console.warn("dragDropEvent no disponible:", err));
    } catch (err) {
      console.warn("dragDropEvent no disponible:", err);
    }

    return () => {
      desmontado = true;
      unlisten?.();
      resizeObserver.disconnect();
    };
  });
</script>

<svelte:window onkeydown={alTeclearVentana} />

<div class="ventana-flotante" style="left: {pos.x}px; top: {pos.y}px;">
  <div class="cabecera-flotante" role="presentation" onpointerdown={iniciarArrastre}>
    <span class="icono-drag">⋮⋮</span>
    <span class="titulo-ventana">
      {entidad.id === 0 ? "Nueva Ficha" : `Ficha: ${entidad.nombre}`}
    </span>
    <button type="button" class="btn-cerrar" onclick={() => partida.cerrarEditorEntidad()}>✕</button>
  </div>

  <div class="cuerpo-ficha">
    <div class="fila-superior">
      <div
        class="caja-foto"
        role="button"
        tabindex="0"
        oncontextmenu={seleccionarFoto}
        title="Clic derecho para elegir imagen o arrastra una aquí"
      >
        {#if entidad.foto_url}
          <img src={urlDeImagen(entidad.foto_url)} alt="Foto ficha" class="img-preview" draggable="false" />
        {:else}
          <div class="foto-placeholder">
            <span>Foto</span>
            <small>(público siempre)</small>
          </div>
        {/if}
      </div>

      <div class="columna-campos">
        <div class="campo-fila">
          <label for="f-nombre">Nombre:</label>
          <input id="f-nombre" type="text" class="input-texto" bind:value={entidad.nombre} />
        </div>

        <div class="campo-fila fila-stats">
          <label>Vida max: <input type="number" class="input-num" bind:value={entidad.vida_max} /></label>
          <label>Armadura: <input type="number" class="input-num" bind:value={entidad.armadura} /></label>
        </div>

        <div class="campo-fila fila-checks">
          <label class="check-publico">
            <span>Hacer pública la vida</span>
            <input type="checkbox" bind:checked={entidad.vida_es_publica} />
          </label>
          <label class="check-publico">
            <span>Hacer pública la armadura</span>
            <input type="checkbox" bind:checked={entidad.armadura_es_publica} />
          </label>
        </div>

        <div class="campo-fila">
          <label for="f-tipo">Tipo:</label>
          <select id="f-tipo" class="select-general" bind:value={entidad.tipo}>
            <option value="Enemigo">Enemigo</option>
            <option value="NPC">NPC</option>
            <option value="Jugador">Jugador</option>
          </select>

          <label class="check-publico" style="margin-left: 8px;">
            <input type="checkbox" bind:checked={entidad.es_unico} />
            <span title="Si es único solo existirá una instancia en el mundo">Único</span>
          </label>
        </div>

        <div class="campo-fila">
          <label for="f-zona">Zona asociada:</label>
          <select id="f-zona" class="select-general" bind:value={entidad.zona_id}>
            {#each partida.zonasOrdenadas as z}
              <option value={z.id}>{z.nombre}</option>
            {/each}
          </select>
        </div>
      </div>
    </div>

    <!-- Zona de Archivos Adjuntos -->
    <div class="zona-archivos">
      <div class="barra-pestanas-wrapper">
        {#if puedeScrollIzq}
          <button type="button" class="btn-flecha-scroll" onclick={scrollIzquierda} title="Desplazar a la izquierda">
            ◀
          </button>
        {/if}

        <div class="contenedor-tabs-scroll" bind:this={barraTabs} onscroll={comprobarScroll}>
          {#each entidad.archivos_adjuntos as archivo, i}
            {#if archivo}
              <button
                type="button"
                class="btn-pestana"
                class:activa={pestanaActiva === i}
                onclick={() => cargarContenidoPestana(i)}
                title="Doble clic para renombrar archivo"
              >
                {#if editandoTabIndice === i}
                  <input
                    type="text"
                    class="input-renombrar-tab"
                    bind:value={textoEdicionTab}
                    onblur={() => confirmarRenombrado(i)}
                    onkeydown={(e) => {
                      if (e.key === "Enter") {
                        e.stopPropagation();
                        confirmarRenombrado(i);
                      }
                      if (e.key === "Escape") {
                        e.stopPropagation();
                        editandoTabIndice = null;
                      }
                    }}
                    onclick={(e) => e.stopPropagation()}
                    autofocus
                  />
                {:else}
                  <span
                    class="nombre-tab"
                    ondblclick={(e) => {
                      e.stopPropagation();
                      iniciarRenombrado(i, archivo);
                    }}
                  >
                    {nombreLimpio(archivo)}
                  </span>
                {/if}

                <span
                  class="btn-cerrar-tab"
                  role="button"
                  tabindex="0"
                  onclick={(e) => { e.stopPropagation(); desasociarAdjunto(i); }}
                >×</span>
              </button>
            {/if}
          {/each}
        </div>

        {#if puedeScrollDer}
          <button type="button" class="btn-flecha-scroll" onclick={scrollDerecha} title="Desplazar a la derecha">
            ▶
          </button>
        {/if}

        <button type="button" class="btn-anadir-adjunto" onclick={crearNuevoTxt} title="Crear documento .txt">+</button>
      </div>

      <div class="visor-archivo">
        {#if entidad.archivos_adjuntos.length === 0}
          <div class="archivo-placeholder">
            <span>Sin archivos adjuntos. Arrastra archivos aquí o pulsa + para crear uno.</span>
          </div>
        {:else if esTexto(entidad.archivos_adjuntos[pestanaActiva])}
          <textarea
            class="editor-texto"
            placeholder="Notas privadas..."
            value={textoActual}
            oninput={alModificarTexto}
          ></textarea>
        {:else}
          <div class="archivo-placeholder">
            <span>📎 {nombreLimpio(entidad.archivos_adjuntos[pestanaActiva])}</span>
            <small>(Archivo no editable como texto)</small>
          </div>
        {/if}
      </div>
    </div>

    <div class="pie-acciones">
      {#if entidad.id !== 0}
        <button type="button" class="btn-borrar" onclick={() => partida.borrarEntidad(entidad)}>
          Borrar
        </button>
      {/if}
      <div class="botones-derecha">
        <button type="button" class="btn-cancelar" onclick={() => partida.cerrarEditorEntidad()}>
          Cancelar
        </button>
        <button type="button" class="btn-guardar" onclick={guardarTodo}>
          Guardar
        </button>
      </div>
    </div>
  </div>
</div>

<style>
  .ventana-flotante {
    position: fixed;
    width: 460px;
    background: #18181b;
    border: 1px solid #3f3f46;
    border-radius: 8px;
    box-shadow: 0 14px 35px rgba(0, 0, 0, 0.75);
    z-index: 1000;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    user-select: none;
    font-size: 11px;
    color: #f4f4f5;
  }
  .cabecera-flotante {
    background: #27272a;
    padding: 6px 10px;
    display: flex;
    align-items: center;
    gap: 6px;
    cursor: grab;
    border-bottom: 1px solid #3f3f46;
  }
  .cabecera-flotante:active { cursor: grabbing; }
  .icono-drag { color: #71717a; font-size: 12px; }
  .titulo-ventana { font-weight: bold; flex: 1; }
  .btn-cerrar { background: none; border: none; color: #a1a1aa; cursor: pointer; font-size: 13px; }
  .btn-cerrar:hover { color: white; }

  .cuerpo-ficha { padding: 10px; display: flex; flex-direction: column; gap: 10px; }
  .fila-superior { display: flex; gap: 10px; }

  .caja-foto {
    width: 105px;
    height: 105px;
    background: #09090b;
    border: 1px dashed #52525b;
    border-radius: 6px;
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    cursor: pointer;
  }
  .caja-foto:hover { border-color: #3b82f6; }
  .img-preview { width: 100%; height: 100%; object-fit: cover; }
  .foto-placeholder { text-align: center; color: #71717a; font-size: 11px; }
  .foto-placeholder small { display: block; font-size: 8px; margin-top: 2px; }

  .columna-campos { flex: 1; display: flex; flex-direction: column; gap: 4px; }
  .campo-fila { display: flex; align-items: center; gap: 6px; }
  .campo-fila label { color: #a1a1aa; font-size: 10px; }
  .fila-stats { display: flex; gap: 12px; }
  
  .fila-checks {
    display: flex;
    gap: 12px;
    padding: 1px 0;
  }
  .check-publico {
    display: flex;
    align-items: center;
    gap: 4px;
    cursor: pointer;
    font-size: 10px;
    color: #d4d4d8;
  }
  .check-publico input { cursor: pointer; accent-color: #3b82f6; margin: 0; }

  .input-texto {
    flex: 1; background: #09090b; border: 1px solid #3f3f46; color: white;
    padding: 3px 6px; border-radius: 4px; font-size: 11px;
  }
  .input-num {
    width: 44px; background: #09090b; border: 1px solid #3f3f46; color: white;
    padding: 2px 4px; border-radius: 4px; font-size: 11px; margin-left: 4px;
  }
  .select-general {
    background: #09090b; border: 1px solid #3f3f46; color: white;
    padding: 2px 4px; border-radius: 4px; font-size: 10px;
  }

  .zona-archivos {
    border: 1px solid #27272a;
    border-radius: 6px;
    background: #111113;
    overflow: hidden;
  }
  .barra-pestanas-wrapper {
    display: flex;
    background: #1f1f23;
    border-bottom: 1px solid #27272a;
    align-items: center;
    height: 28px;
  }
  .contenedor-tabs-scroll {
    display: flex;
    flex: 1;
    overflow-x: auto;
    scrollbar-width: none;
    height: 100%;
  }
  .contenedor-tabs-scroll::-webkit-scrollbar { display: none; }

  .btn-pestana {
    display: flex;
    align-items: center;
    gap: 5px;
    background: none;
    border: none;
    color: #a1a1aa;
    padding: 0 8px;
    font-size: 10px;
    cursor: pointer;
    border-right: 1px solid #27272a;
    white-space: nowrap;
    height: 100%;
    flex-shrink: 0;
  }
  .btn-pestana.activa { background: #111113; color: white; font-weight: bold; }
  
  .nombre-tab {
    cursor: pointer;
  }

  .input-renombrar-tab {
    background: #09090b;
    border: 1px solid #3b82f6;
    color: white;
    font-size: 10px;
    padding: 1px 3px;
    border-radius: 2px;
    outline: none;
    width: 90px;
  }

  .btn-cerrar-tab {
    color: #71717a;
    font-size: 11px;
    padding: 0 2px;
  }
  .btn-cerrar-tab:hover { color: #ef4444; }

  .btn-flecha-scroll {
    background: #27272a;
    border: none;
    color: #e4e4e7;
    font-size: 8px;
    padding: 0 6px;
    height: 100%;
    cursor: pointer;
    flex-shrink: 0;
    z-index: 2;
  }
  .btn-flecha-scroll:hover { background: #3f3f46; color: white; }

  .btn-anadir-adjunto {
    background: none; border: none; color: #a1a1aa; padding: 0 8px;
    cursor: pointer; font-size: 13px; font-weight: bold; height: 100%; flex-shrink: 0;
  }
  .btn-anadir-adjunto:hover { color: white; background: #27272a; }

  .visor-archivo { height: 105px; padding: 4px; }
  .editor-texto {
    width: 100%; height: 100%; background: transparent; border: none;
    color: #e4e4e7; font-family: inherit; font-size: 11px; resize: none; outline: none;
    box-sizing: border-box;
  }
  .archivo-placeholder {
    height: 100%; display: flex; flex-direction: column; align-items: center;
    justify-content: center; color: #71717a; gap: 4px; text-align: center; font-size: 11px;
  }

  .pie-acciones { display: flex; justify-content: space-between; align-items: center; }
  .botones-derecha { display: flex; gap: 6px; margin-left: auto; }
  .btn-cancelar, .btn-guardar, .btn-borrar {
    border: none; padding: 5px 12px; border-radius: 4px; font-size: 11px; font-weight: 600; cursor: pointer;
  }
  .btn-cancelar { background: #27272a; color: #d4d4d8; }
  .btn-guardar { background: #2563eb; color: white; }
  .btn-borrar { background: #7f1d1d; color: #fca5a5; }
  .btn-cancelar:hover { background: #3f3f46; }
  .btn-guardar:hover { background: #1d4ed8; }
  .btn-borrar:hover { background: #991b1b; }
</style>