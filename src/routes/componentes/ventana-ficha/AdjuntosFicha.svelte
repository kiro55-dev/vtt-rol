<!-- src/routes/componentes/ventana-ficha/AdjuntosFicha.svelte -->
<!-- Pestañas de los archivos adjuntos de una ficha y el visor del que está abierto. Soltar
     archivos sobre `.zona-archivos` los adjunta (lo gestiona VentanaFicha). -->
<script lang="ts">
  import { ayuda } from "$lib/ajustesVista.svelte";
  import { tick } from "svelte";
  import { esTexto, nombreArchivo, type EditorFicha } from "$lib/editorFicha.svelte";
  import { esJugador } from "$lib/modo";

  let { editor }: { editor: EditorFicha } = $props();

  let barra = $state<HTMLDivElement | null>(null);
  let puedeIzq = $state(false);
  let puedeDer = $state(false);

  // Renombrado con doble clic
  let editando = $state<number | null>(null);
  let textoEdicion = $state("");

  const ruta = $derived(editor.rutaActiva);

  function comprobarScroll() {
    if (!barra) return;
    puedeIzq = barra.scrollLeft > 2;
    puedeDer = barra.scrollLeft + barra.clientWidth < barra.scrollWidth - 2;
  }

  const desplazar = (dx: number) => barra?.scrollBy({ left: dx, behavior: "smooth" });

  // Cambia el tamaño de la barra -> recalcular las flechas
  $effect(() => {
    if (!barra) return;
    const observador = new ResizeObserver(comprobarScroll);
    observador.observe(barra);
    return () => observador.disconnect();
  });

  // Cambian las pestañas (añadir, quitar, renombrar) o la activa -> flechas y activa a la vista
  $effect(() => {
    void [...editor.adjuntos];
    void editor.pestana;
    tick().then(() => {
      barra?.querySelector(".pestana.activa")?.scrollIntoView({ block: "nearest", inline: "nearest" });
      comprobarScroll();
    });
  });

  function empezarRenombrado(i: number, archivo: string) {
    editando = i;
    textoEdicion = nombreArchivo(archivo);
  }

  function confirmarRenombrado(i: number) {
    if (editando === null) return;
    editando = null;
    editor.renombrar(i, textoEdicion);
  }

  function teclaRenombrado(e: KeyboardEvent, i: number) {
    // Nada sale del input: ni Enter/Escape a la ventana (guardar/cerrar) ni el espacio a la pestaña
    e.stopPropagation();
    if (e.key === "Enter") confirmarRenombrado(i);
    else if (e.key === "Escape") editando = null;
  }

  /** Enfoca el input y selecciona el nombre sin la extensión. */
  function enfocar(input: HTMLInputElement) {
    input.focus();
    const punto = input.value.lastIndexOf(".");
    input.setSelectionRange(0, punto > 0 ? punto : input.value.length);
  }
</script>

<div class="zona-archivos">
  <div class="barra-pestanas">
    {#if puedeIzq}
      <button type="button" class="btn-flecha" onclick={() => desplazar(-100)} title={ayuda("Desplazar a la izquierda")}>◀</button>
    {/if}

    <div class="pestanas" role="tablist" bind:this={barra} onscroll={comprobarScroll}>
      {#each editor.adjuntos as archivo, i}
        {#if archivo}
          <div
            class="pestana"
            class:activa={editor.pestana === i}
            role="tab"
            tabindex="0"
            aria-selected={editor.pestana === i}
            title={ayuda("Doble clic para renombrar archivo")}
            onclick={() => editor.abrirPestana(i)}
            onkeydown={(e) => {
              if (e.key === " ") {
                e.preventDefault();
                editor.abrirPestana(i);
              }
            }}
            ondblclick={() => empezarRenombrado(i, archivo)}
          >
            {#if editando === i}
              <input
                type="text"
                class="input-renombrar"
                maxlength="80"
                bind:value={textoEdicion}
                use:enfocar
                onblur={() => confirmarRenombrado(i)}
                onkeydown={(e) => teclaRenombrado(e, i)}
                onclick={(e) => e.stopPropagation()}
                ondblclick={(e) => e.stopPropagation()}
              />
            {:else}
              <span>{nombreArchivo(archivo)}</span>
            {/if}
            <button
              type="button"
              class="btn-quitar"
              title={ayuda("Quitar de la ficha")}
              aria-label="Quitar {nombreArchivo(archivo)}"
              onclick={(e) => {
                e.stopPropagation();
                editor.quitar(i);
              }}>×</button
            >
          </div>
        {/if}
      {/each}
    </div>

    {#if puedeDer}
      <button type="button" class="btn-flecha" onclick={() => desplazar(100)} title={ayuda("Desplazar a la derecha")}>▶</button>
    {/if}

    <button type="button" class="btn-anadir" onclick={() => editor.crearTexto()} title={ayuda("Crear documento .txt")}>+</button>
  </div>

  <div class="visor">
    {#if editor.adjuntos.length === 0}
      <div class="placeholder">
        <span>Sin archivos adjuntos. Arrastra archivos aquí o pulsa + para crear uno.</span>
      </div>
    {:else if ruta && esTexto(ruta)}
      {#if ruta in editor.textos}
        <textarea
          placeholder="Notas privadas..."
          value={editor.textos[ruta]}
          oninput={(e) => editor.escribir(e.currentTarget.value)}
        ></textarea>
      {:else}
        <div class="placeholder">
          <small>
            {editor.lecturaFallida(ruta) ? "No se pudo leer el archivo. Pulsa su pestaña para reintentar." : "Cargando…"}
          </small>
        </div>
      {/if}
    {:else if ruta}
      <div class="placeholder">
        <span class="nombre-centro">{nombreArchivo(ruta)}</span>
        <small>(Archivo no editable como texto)</small>
        {#if esJugador}
          <small>Solo el host puede abrir este tipo de archivo.</small>
        {:else}
          <button type="button" class="btn-abrir" onclick={() => editor.abrirEnSistema(ruta)}>
            Abrir en el visor del sistema
          </button>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .zona-archivos { border: 1px solid #27272a; border-radius: 6px; background: #111113; overflow: hidden; }

  .barra-pestanas {
    display: flex; align-items: center; height: 28px;
    background: #1f1f23; border-bottom: 1px solid #27272a;
  }
  .pestanas { display: flex; flex: 1; height: 100%; overflow-x: auto; scrollbar-width: none; }
  .pestanas::-webkit-scrollbar { display: none; }

  .pestana {
    display: flex; align-items: center; gap: 5px; flex-shrink: 0; height: 100%;
    padding: 0 8px; border-right: 1px solid #27272a;
    color: #a1a1aa; font-size: 10px; white-space: nowrap; cursor: pointer;
  }
  .pestana.activa { background: #111113; color: white; font-weight: bold; }

  .input-renombrar {
    width: 90px; padding: 1px 3px; border-radius: 2px; outline: none;
    background: #09090b; border: 1px solid #3b82f6; color: white; font-size: 10px;
  }

  .btn-quitar {
    background: none; border: none; padding: 0 2px; cursor: pointer;
    color: #71717a; font-size: 11px;
  }
  .btn-quitar:hover { color: #ef4444; }

  .btn-flecha {
    flex-shrink: 0; height: 100%; padding: 0 6px; z-index: 2;
    background: #27272a; border: none; color: #e4e4e7; font-size: 8px; cursor: pointer;
  }
  .btn-flecha:hover { background: #3f3f46; color: white; }

  .btn-anadir {
    flex-shrink: 0; height: 100%; padding: 0 8px;
    background: none; border: none; color: #a1a1aa; font-size: 13px; font-weight: bold; cursor: pointer;
  }
  .btn-anadir:hover { color: white; background: #27272a; }

  .visor { height: 100px; padding: 4px; }
  textarea {
    width: 100%; height: 100%; box-sizing: border-box; resize: none; outline: none;
    background: transparent; border: none; color: #e4e4e7; font-family: inherit; font-size: 11px;
  }
  .placeholder {
    height: 100%; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 4px;
    color: #a1a1aa; font-size: 11px; text-align: center;
  }
  .placeholder small { color: #71717a; font-size: 10px; }
  .nombre-centro { color: #f4f4f5; font-weight: 500; }

  .btn-abrir {
    margin-top: 4px; padding: 4px 10px; border-radius: 4px; cursor: pointer;
    background: #27272a; border: 1px solid #3f3f46; color: #60a5fa; font-size: 10px; font-weight: 500;
    transition: background 0.15s ease, color 0.15s ease;
  }
  .btn-abrir:hover { background: #3f3f46; color: white; }
</style>
