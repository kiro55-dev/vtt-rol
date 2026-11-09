<!-- src/routes/componentes/panel-izquierdo/ListaZonas.svelte -->
<script lang="ts">
  import { ayuda } from "$lib/ajustesVista.svelte";
  import { flip } from "svelte/animate";
  import { partida, type Zona } from "$lib/partida.svelte";
  import { ListaOrdenable } from "$lib/ordenable.svelte";
  import { esJugador } from "$lib/modo";
  import MenuContextual from "../comunes/MenuContextual.svelte";
  import ArrastreFlotante from "../comunes/ArrastreFlotante.svelte";

  let editandoId = $state<number | null>(null);
  let textoEdicion = $state("");
  let menu = $state<{ x: number; y: number; zona: Zona } | null>(null);

  const lista = new ListaOrdenable<Zona>({
    selector: ".item-zona",
    alReordenar: (ids) => partida.reordenarZonas(ids),
    alPulsar: (z) => partida.seleccionarZona(z.id),
    bloqueado: () => editandoId !== null,
    soloClic: esJugador // el jugador solo abre zonas: ni las reordena ni las renombra
  });

  $effect(() => lista.sincronizar(partida.zonas));

  function iniciarEdicion(z: Zona) {
    menu = null;
    editandoId = z.id;
    textoEdicion = z.nombre;
  }

  async function guardarEdicion(z: Zona) {
    if (editandoId === null) return;
    editandoId = null;
    const nombre = textoEdicion.trim() || z.nombre;
    if (nombre !== z.nombre) await partida.renombrarZona(z, nombre);
  }

  function abrirMenu(e: MouseEvent, zona: Zona) {
    e.preventDefault();
    menu = { x: e.clientX, y: e.clientY, zona };
  }
</script>

<div class="lista">
  <span class="titulo-mini">ZONAS</span>
  <div class="scroll">
    {#each lista.visuales as z, i (z.id)}
      <div animate:flip={{ duration: 150 }}>
        <button
          class="item-zona"
          class:activo={partida.zonaId === z.id}
          class:fantasma-origen={lista.activo && lista.item?.id === z.id}
          data-indice={i}
          onpointerdown={(e) => lista.iniciar(z, i, e)}
          ondblclick={(e) => {
            e.stopPropagation();
            if (!esJugador) iniciarEdicion(z);
          }}
          oncontextmenu={(e) => (esJugador ? e.preventDefault() : abrirMenu(e, z))}
          title={ayuda(z.nombre)}
        >
          {#if editandoId === z.id}
            <input
              type="text"
              class="input-nombre"
              bind:value={textoEdicion}
              onblur={() => guardarEdicion(z)}
              onkeydown={(e) => {
                if (e.key === "Enter") guardarEdicion(z);
                if (e.key === "Escape") editandoId = null;
              }}
              onclick={(e) => e.stopPropagation()}
              autofocus
            />
          {:else}
            <span class="texto-recortado">{z.nombre}</span>
          {/if}
        </button>
      </div>
    {/each}
    {#if !esJugador}
      <button class="item-zona nueva" onclick={() => partida.crearZona()} title={ayuda("Añadir zona")}>+</button>
    {/if}
  </div>
</div>

<ArrastreFlotante {lista}>
  {#if lista.item}
    <div class="item-zona clon activo">
      <span class="texto-recortado">{lista.item.nombre}</span>
    </div>
  {/if}
</ArrastreFlotante>

{#if menu}
  {@const zona = menu.zona}
  <MenuContextual
    x={menu.x}
    y={menu.y}
    onCerrar={() => (menu = null)}
    opciones={[
      { texto: "Cambiar nombre", accion: () => iniciarEdicion(zona) },
      { texto: "Borrar zona", peligro: true, accion: () => partida.borrarZona(zona) }
    ]}
  />
{/if}

<style>
  .lista { flex: 1; min-height: 0; display: flex; flex-direction: column; }
  .titulo-mini { font-size: 10px; font-weight: bold; color: #71717a; margin-bottom: 3px; display: block; }
  .scroll { flex: 1; overflow-y: auto; display: flex; flex-direction: column; gap: 3px; }

  .item-zona {
    width: 100%; position: relative; background: #27272a; border: 1px solid #3f3f46;
    color: white; font-weight: 500; padding: 6px 4px; border-radius: 4px; cursor: grab;
    text-align: center; overflow: hidden; touch-action: none;
  }
  .item-zona:active { cursor: grabbing; }
  .item-zona.activo { background: #3b82f6; border-color: #60a5fa; }
  .item-zona.nueva { border-style: dashed; font-size: 14px; padding: 3px; cursor: pointer; }

  /* Efecto hueco punteado mientras se arrastra */
  .fantasma-origen {
    opacity: 0.25 !important;
    border: 1px dashed #60a5fa !important;
    transform: scale(0.96);
  }

  .texto-recortado { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  .input-nombre {
    width: 90%; background: #09090b; border: 1px solid #60a5fa; color: white;
    font-size: 10px; padding: 2px; border-radius: 2px; text-align: center; outline: none;
  }

  .clon { cursor: grabbing !important; }
</style>
