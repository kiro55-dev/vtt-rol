<!-- src/routes/componentes/VentanaFicha.svelte -->
<!-- Ventana flotante (no modal) para crear o editar una ficha. Aquí solo está la vista: la lógica
     (archivos de la sesión, textos, validación, guardar/cancelar/borrar) vive en $lib/editorFicha. -->
<script lang="ts">
  import { onMount } from "svelte";
  import { partida, type Entidad } from "$lib/partida.svelte";
  import { EditorFicha } from "$lib/editorFicha.svelte";
  import { ES_IMAGEN } from "$lib/imagenes";
  import { alSoltarArchivos } from "$lib/soltarArchivos";
  import { moverVentana } from "$lib/moverVentana";
  import CajaFoto from "./ventana-ficha/CajaFoto.svelte";
  import CampoNumero from "./ventana-ficha/CampoNumero.svelte";
  import AdjuntosFicha from "./ventana-ficha/AdjuntosFicha.svelte";

  let { entidad }: { entidad: Entidad } = $props();

  // +page monta la ventana de nuevo para cada ficha ({#if}), así que basta con crearlo una vez.
  // svelte-ignore state_referenced_locally
  const editor = new EditorFicha(entidad, partida.editorEsBorrador);
  const ficha = editor.entidad;
  // Las definiciones no cambian mientras la ventana está abierta
  const campos = editor.campos;

  let ventana = $state<HTMLDivElement | null>(null);
  let pos = $state({ x: 260, y: 70 });

  function alTeclear(e: KeyboardEvent) {
    if (e.key === "Enter") {
      if ((e.target as HTMLElement)?.tagName === "TEXTAREA") return;
      e.preventDefault();
      editor.guardar();
    } else if (e.key === "Escape") {
      editor.cancelar();
    }
  }


  onMount(() => {
    ventana?.focus(); // para que Enter/Escape funcionen sin tocar la ventana con el ratón
    editor.abrirPestana(0);

    return alSoltarArchivos((rutas, el) => {
      if (el?.closest(".caja-foto")) {
        const imagen = rutas.find((r) => ES_IMAGEN.test(r));
        if (imagen) return editor.ponerFoto(imagen);
      } else if (el?.closest(".zona-archivos")) {
        return editor.importarArchivos(rutas);
      }
    });
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  class="ventana-flotante"
  style="left: {pos.x}px; top: {pos.y}px;"
  role="dialog"
  aria-label="Ficha"
  tabindex="-1"
  onkeydown={alTeclear}
  bind:this={ventana}
>
  <div class="cabecera" role="presentation" onpointerdown={(e) => moverVentana(e, pos, ventana)}>
    <span class="icono-drag">⋮⋮</span>
    <span class="titulo">{editor.esBorrador ? "Nueva Ficha" : `Ficha: ${ficha.nombre}`}</span>
    <button type="button" class="btn-cerrar" aria-label="Cerrar" onclick={() => editor.cancelar()}>✕</button>
  </div>

  <div class="cuerpo-ficha">
    <div class="fila-superior">
      <CajaFoto foto={ficha.foto_url} onElegir={(origen) => editor.ponerFoto(origen)} />

      <div class="columna-campos">
        <div class="fila">
          <label for="f-nombre">Nombre:</label>
          <input id="f-nombre" type="text" class="input-texto" bind:value={ficha.nombre} />
        </div>

        <!-- Parámetros de la partida: los numéricos de dos en dos y, debajo de cada pareja, las
             casillas de los que cada ficha puede hacer públicos. Luego los de texto. -->
        {#each campos.numericos as par}
          <div class="fila separada">
            {#each par as d (d.id)}
              <CampoNumero
                etiqueta="{d.clave}{d.tiene_maximo ? ' max' : ''}:"
                sufijo={d.unidad ?? ""}
                bind:value={() => editor.numero(d.id), (v) => editor.ponerNumero(d.id, v)}
              />
            {/each}
          </div>

          {#if par.some((d) => d.visibilidad === "Variable")}
            <div class="fila separada">
              {#each par.filter((d) => d.visibilidad === "Variable") as d (d.id)}
                <label class="check">
                  <span>Hacer pública la {d.clave.toLowerCase()}</span>
                  <input
                    type="checkbox"
                    bind:checked={() => editor.esPublico(d.id), (v) => editor.ponerPublico(d.id, v)}
                  />
                </label>
              {/each}
            </div>
          {/if}
        {/each}

        {#each campos.textos as d (d.id)}
          <div class="fila">
            <label for="f-param-{d.id}">{d.clave}:</label>
            <input
              id="f-param-{d.id}"
              type="text"
              class="input-texto"
              bind:value={() => editor.texto(d.id), (v) => editor.ponerTexto(d.id, v)}
            />
          </div>
        {/each}

        <div class="fila">
          <span class="etiqueta">Cuerpo:</span>
          <div class="cuerpo">
            <CampoNumero bind:value={editor.cuerpoX} min={1} max={255} estrecho titulo="Ancho en casillas (rueda del ratón para ajustar)" />
            <span class="separador-x">×</span>
            <CampoNumero bind:value={editor.cuerpoY} min={1} max={255} estrecho sufijo="m" titulo="Alto en casillas (rueda del ratón para ajustar)" />
          </div>
        </div>

        <div class="fila">
          <label for="f-tipo">Tipo:</label>
          <select id="f-tipo" class="select" bind:value={ficha.tipo}>
            <option value="Enemigo">Enemigo</option>
            <option value="NPC">NPC</option>
            <option value="Jugador">Jugador</option>
          </select>

          <label class="check" style="margin-left: 8px;">
            <input type="checkbox" bind:checked={ficha.es_unico} />
            <span title="Si es único solo existirá una instancia en el mundo">Único</span>
          </label>
        </div>

        <div class="fila">
          <label for="f-zona">Zona asociada:</label>
          <select id="f-zona" class="select" bind:value={ficha.zona_id}>
            {#each partida.zonasOrdenadas as z (z.id)}
              <option value={z.id}>{z.nombre}</option>
            {/each}
          </select>
        </div>
      </div>
    </div>

    <AdjuntosFicha {editor} />

    {#if editor.error}
      <p class="error" role="alert">{editor.error}</p>
    {/if}

    <div class="pie">
      {#if !editor.esBorrador}
        <button type="button" class="btn-borrar" onclick={() => editor.borrar()} disabled={editor.guardando}>Borrar</button>
      {/if}
      <div class="botones-derecha">
        <button type="button" class="btn-cancelar" onclick={() => editor.cancelar()}>Cancelar</button>
        <button type="button" class="btn-guardar" onclick={() => editor.guardar()} disabled={editor.guardando}>Guardar</button>
      </div>
    </div>
  </div>
</div>

<style>
  .ventana-flotante {
    position: fixed; z-index: 1000; width: 480px;
    display: flex; flex-direction: column; overflow: hidden;
    background: #18181b; border: 1px solid #3f3f46; border-radius: 8px;
    box-shadow: 0 14px 35px rgba(0, 0, 0, 0.75);
    user-select: none; font-size: 11px; color: #f4f4f5;
  }

  .cabecera {
    display: flex; align-items: center; gap: 6px; padding: 6px 10px; cursor: grab;
    background: #27272a; border-bottom: 1px solid #3f3f46;
  }
  .cabecera:active { cursor: grabbing; }
  .icono-drag { color: #71717a; font-size: 12px; }
  .titulo { font-weight: bold; flex: 1; }
  .btn-cerrar { background: none; border: none; color: #a1a1aa; cursor: pointer; font-size: 13px; }
  .btn-cerrar:hover { color: white; }

  .cuerpo-ficha { padding: 10px; display: flex; flex-direction: column; gap: 8px; }
  .fila-superior { display: flex; gap: 10px; align-items: flex-start; }
  .columna-campos { flex: 1; display: flex; flex-direction: column; gap: 4px; }

  .fila { display: flex; align-items: center; gap: 6px; }
  .fila.separada { gap: 10px; }
  .fila label, .etiqueta { color: #a1a1aa; font-size: 10px; display: flex; align-items: center; gap: 4px; }

  .check { cursor: pointer; }
  .check input { cursor: pointer; accent-color: #3b82f6; margin: 0; }

  .cuerpo { display: flex; align-items: center; gap: 4px; }
  .separador-x { color: #71717a; font-weight: bold; font-size: 11px; }

  .input-texto {
    flex: 1; padding: 3px 6px; border-radius: 4px;
    background: #09090b; border: 1px solid #3f3f46; color: white; font-size: 11px;
  }
  .select {
    padding: 2px 4px; border-radius: 4px;
    background: #09090b; border: 1px solid #3f3f46; color: white; font-size: 10px;
  }

  .error { margin: 0 0 6px; color: #f87171; font-size: 12px; }

  .pie { display: flex; justify-content: space-between; align-items: center; }
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
