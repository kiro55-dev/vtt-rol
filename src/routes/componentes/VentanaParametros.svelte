<!-- src/routes/componentes/VentanaParametros.svelte -->
<!-- Ventana flotante (no modal) para editar los parámetros de la partida: los que tienen todas las
     fichas (vida, armadura...). Se pueden crear, borrar y reordenar arrastrando; el orden de la lista
     es el que siguen las fichas. -->
<script lang="ts">
  import { flip } from "svelte/animate";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { partida, type DefinicionParametro } from "$lib/partida.svelte";
  import { ListaOrdenable } from "$lib/ordenable.svelte";
  import { moverVentana } from "$lib/moverVentana";
  import { FormularioParametro, formasDisponibles, VISIBILIDADES, resumenDefinicion } from "$lib/formularioParametro.svelte";
  import ArrastreFlotante from "./comunes/ArrastreFlotante.svelte";

  let { onCerrar }: { onCerrar: () => void } = $props();

  type Fila = DefinicionParametro & { orden: number };

  let ventana = $state<HTMLDivElement | null>(null);
  let pos = $state({ x: 260, y: 70 });
  let error = $state("");
  let creando = $state(false);
  let guardando = $state(false);
  const formulario = new FormularioParametro();

  const lista = new ListaOrdenable<Fila>({
    selector: ".fila-param",
    alReordenar: async (ids) => {
      error = "";
      try {
        await partida.reordenarDefiniciones(ids);
      } catch (err) {
        error = String(err); // la lista vuelve sola al orden real al recargar
      }
    },
    alPulsar: (d) => editar(d)
  });

  // El orden de la lista de definiciones ES el orden: se numera para la lista ordenable
  $effect(() => lista.sincronizar(partida.definiciones.map((d, i) => ({ ...d, orden: i }))));

  function editar(d: DefinicionParametro) {
    formulario.cargar(d);
    creando = true;
    error = "";
  }

  function nuevo() {
    formulario.reiniciar();
    creando = true;
    error = "";
  }

  function cancelarFormulario() {
    formulario.reiniciar();
    creando = false;
    error = "";
  }

  async function borrar(d: DefinicionParametro) {
    const seguro = await ask(`¿Borrar el parámetro «${d.clave}»? Desaparecerá de todas las fichas.`, {
      title: "Borrar parámetro",
      kind: "warning"
    });
    if (!seguro) return;
    error = "";
    try {
      await partida.borrarDefinicion(d.id);
    } catch (err) {
      error = String(err);
    }
  }

  async function guardar() {
    if (guardando) return;
    guardando = true;
    error = "";
    try {
      const definicion = formulario.aDefinicion();
      if (formulario.idEditando !== null) await partida.modificarDefinicion({ ...definicion, id: formulario.idEditando });
      else await partida.crearDefinicion(definicion);
      formulario.reiniciar();
      creando = false;
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      guardando = false;
    }
  }

  function alTeclear(e: KeyboardEvent) {
    if (e.key === "Escape") onCerrar();
    else if (e.key === "Enter" && creando && (e.target as HTMLElement).tagName === "INPUT") {
      e.preventDefault();
      guardar();
    }
  }

  $effect(() => ventana?.focus()); // para que Escape funcione sin tocar la ventana
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  class="ventana-flotante"
  style="left: {pos.x}px; top: {pos.y}px;"
  role="dialog"
  aria-label="Parámetros de la partida"
  tabindex="-1"
  onkeydown={alTeclear}
  bind:this={ventana}
>
  <div class="cabecera" role="presentation" onpointerdown={(e) => moverVentana(e, pos, ventana)}>
    <span class="icono-drag">⋮⋮</span>
    <span class="titulo">Parámetros de esta partida</span>
    <button type="button" class="btn-cerrar" aria-label="Cerrar" onclick={onCerrar}>✕</button>
  </div>

  <div class="cuerpo">
    <p class="ayuda">Los tienen todas las fichas. Pulsa uno para configurarlo y arrástralo para cambiar el orden en que salen en ellas.</p>

    <div class="lista">
      {#each lista.visuales as d, i (d.id)}
        <div animate:flip={{ duration: 150 }}>
          <div
            class="fila-param"
            class:fantasma-origen={lista.activo && lista.item?.id === d.id}
            data-indice={i}
            role="presentation"
            onpointerdown={(e) => lista.iniciar(d, i, e)}
          >
            <span class="icono-drag">⋮⋮</span>
            <div class="textos">
              <span class="clave">{d.clave}</span>
              <span class="resumen">{resumenDefinicion(d)}</span>
            </div>
            <button
              type="button"
              class="btn-editar"
              title="Configurar parámetro"
              aria-label="Configurar {d.clave}"
              onpointerdown={(e) => e.stopPropagation()}
              onclick={() => editar(d)}>✎</button
            >
            <button
              type="button"
              class="btn-borrar"
              title="Borrar parámetro"
              aria-label="Borrar {d.clave}"
              onpointerdown={(e) => e.stopPropagation()}
              onclick={() => borrar(d)}>✕</button
            >
          </div>
        </div>
      {:else}
        <div class="vacio">No hay parámetros: las fichas solo tendrán nombre, tipo, cuerpo y archivos.</div>
      {/each}
    </div>

    {#if creando}
      <div class="formulario">
        <span class="titulo-form">{formulario.idEditando !== null ? "Configurar parámetro" : "Nuevo parámetro"}</span>
        <div class="fila">
          <label for="p-clave">Nombre:</label>
          <input id="p-clave" type="text" class="input-texto" maxlength="30" placeholder="p. ej. Cordura" bind:value={formulario.clave} />
        </div>

        <div class="fila">
          <label for="p-tipo">Tipo:</label>
          <select id="p-tipo" class="select" disabled={formulario.idEditando !== null} title={formulario.idEditando !== null ? "El tipo no se puede cambiar" : undefined} bind:value={formulario.tipo}>
            <option value="Numero">Número</option>
            <option value="Texto">Texto</option>
          </select>

          {#if formulario.esNumero}
            <label for="p-valor">Valor por defecto:</label>
            <input id="p-valor" type="number" class="input-num" min="0" max="32767" bind:value={formulario.valorNumero} />
          {:else}
            <label for="p-valor">Por defecto:</label>
            <input id="p-valor" type="text" class="input-texto" bind:value={formulario.valorTexto} />
          {/if}
        </div>

        {#if formulario.esNumero}
          <div class="fila">
            <label class="check" title="La ficha guarda el máximo y cada instancia en el tablero, su valor actual (como la vida)">
              <input type="checkbox" bind:checked={formulario.tieneMaximo} />
              <span>Tiene máximo (valor actual aparte en cada instancia)</span>
            </label>
          </div>
          <div class="fila">
            <label for="p-unidad">Unidad:</label>
            <input id="p-unidad" type="text" class="input-num" maxlength="8" placeholder="m" bind:value={formulario.unidad} />

            <label for="p-forma">En el tablero:</label>
            <select id="p-forma" class="select" bind:value={formulario.forma}>
              {#each formasDisponibles(formulario.tieneMaximo) as f}<option value={f.valor}>{f.texto}</option>{/each}
            </select>
            {#if formulario.forma !== "ninguna"}
              <input type="color" class="input-color" aria-label="Color de la forma" bind:value={formulario.color} />
            {/if}
          </div>
        {/if}

        <div class="fila">
          <label for="p-vis">Visibilidad:</label>
          <select id="p-vis" class="select" bind:value={formulario.visibilidad}>
            {#each VISIBILIDADES as v}<option value={v.valor}>{v.texto}</option>{/each}
          </select>
        </div>

        <div class="botones">
          <button type="button" class="btn-cancelar" onclick={cancelarFormulario}>Cancelar</button>
          <button type="button" class="btn-guardar" onclick={guardar} disabled={guardando}>{formulario.idEditando !== null ? "Guardar" : "Crear"}</button>
        </div>
      </div>
    {:else}
      <button type="button" class="btn-crear" onclick={nuevo}>+ Crear parámetro</button>
    {/if}

    {#if error}
      <p class="error" role="alert">{error}</p>
    {/if}
  </div>
</div>

<ArrastreFlotante {lista}>
  {#if lista.item}
    <div class="fila-param flotante">
      <span class="icono-drag">⋮⋮</span>
      <div class="textos">
        <span class="clave">{lista.item.clave}</span>
        <span class="resumen">{resumenDefinicion(lista.item)}</span>
      </div>
    </div>
  {/if}
</ArrastreFlotante>

<style>
  .ventana-flotante {
    position: fixed; z-index: 1000; width: 400px;
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

  .cuerpo { padding: 10px; display: flex; flex-direction: column; gap: 8px; }
  .ayuda { margin: 0; color: #a1a1aa; font-size: 10px; }

  .lista { display: flex; flex-direction: column; gap: 3px; max-height: 240px; overflow-y: auto; }
  .fila-param {
    display: flex; align-items: center; gap: 8px; padding: 5px 8px; cursor: grab;
    background: #111113; border: 1px solid #27272a; border-radius: 5px;
  }
  .fila-param:hover { border-color: #3f3f46; }
  .fila-param.fantasma-origen { opacity: 0.3; }
  .fila-param.flotante { height: 100%; box-sizing: border-box; background: #27272a; }
  .textos { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .clave { font-weight: 600; }
  .resumen { color: #71717a; font-size: 9px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .vacio { color: #71717a; font-size: 10px; padding: 8px; text-align: center; }

  .btn-editar { background: none; border: none; color: #71717a; cursor: pointer; font-size: 12px; padding: 2px 4px; }
  .btn-editar:hover { color: #60a5fa; }
  .btn-borrar { background: none; border: none; color: #71717a; cursor: pointer; font-size: 12px; padding: 2px 4px; }
  .btn-borrar:hover { color: #ef4444; }

  .btn-crear {
    padding: 6px; border-radius: 5px; cursor: pointer; font-size: 11px; font-weight: 600;
    background: transparent; border: 1px dashed #52525b; color: #a1a1aa;
  }
  .btn-crear:hover { border-color: #3b82f6; color: white; }

  .formulario {
    display: flex; flex-direction: column; gap: 6px; padding: 8px;
    background: #111113; border: 1px solid #27272a; border-radius: 6px;
  }
  .titulo-form { font-weight: 600; font-size: 10px; color: #d4d4d8; }
  .fila { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .fila label { color: #a1a1aa; font-size: 10px; display: flex; align-items: center; gap: 4px; }
  .check { cursor: pointer; }
  .check input { accent-color: #3b82f6; margin: 0; }

  .input-texto {
    flex: 1; min-width: 80px; padding: 3px 6px; border-radius: 4px;
    background: #09090b; border: 1px solid #3f3f46; color: white; font-size: 11px;
  }
  .input-num {
    width: 54px; padding: 2px 4px; border-radius: 4px;
    background: #09090b; border: 1px solid #3f3f46; color: white; font-size: 11px;
  }
  .select {
    padding: 2px 4px; border-radius: 4px;
    background: #09090b; border: 1px solid #3f3f46; color: white; font-size: 10px;
  }
  .input-color { width: 26px; height: 20px; padding: 0; border: 1px solid #3f3f46; background: none; cursor: pointer; }

  .botones { display: flex; justify-content: flex-end; gap: 6px; }
  .btn-cancelar, .btn-guardar {
    border: none; padding: 5px 12px; border-radius: 4px; font-size: 11px; font-weight: 600; cursor: pointer;
  }
  .btn-cancelar { background: #27272a; color: #d4d4d8; }
  .btn-guardar { background: #2563eb; color: white; }
  .btn-cancelar:hover { background: #3f3f46; }
  .btn-guardar:hover { background: #1d4ed8; }

  .error { margin: 0; color: #f87171; font-size: 12px; }
</style>
