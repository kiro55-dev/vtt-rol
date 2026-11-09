<!-- src/routes/componentes/panel-izquierdo/ListaMapas.svelte -->
<script lang="ts">
  import { onMount } from "svelte";
  import { flip } from "svelte/animate";
  import { open } from "@tauri-apps/plugin-dialog";
  import { partida, type Mapa } from "$lib/partida.svelte";
  import { miniaturas } from "$lib/miniaturas.svelte";
  import { sala } from "$lib/sala.svelte";
  import { ListaOrdenable } from "$lib/ordenable.svelte";
  import { esImagen, EXTENSIONES_IMAGEN } from "$lib/imagenes";
  import { alSoltarArchivos } from "$lib/soltarArchivos";
  import MenuContextual from "../comunes/MenuContextual.svelte";
  import ArrastreFlotante from "../comunes/ArrastreFlotante.svelte";
  import VentanaReemplazoImagen from "./VentanaReemplazoImagen.svelte";

  let editandoId = $state<number | null>(null);
  let textoEdicion = $state("");
  let menu = $state<{ x: number; y: number; mapa: Mapa } | null>(null);
  // Mapa con imagen al que se intenta poner otra: se pide confirmación antes de importarla.
  let reemplazo = $state<{ mapa: Mapa; rutaOrigen: string } | null>(null);

  const lista = new ListaOrdenable<Mapa>({
    selector: ".tarjeta-mapa",
    alReordenar: (ids) => partida.reordenarMapas(ids),
    alPulsar: (m) => partida.seleccionarMapa(m),
    bloqueado: () => editandoId !== null
  });

  $effect(() => lista.sincronizar(partida.mapas));

  // Genera las miniaturas que falten cada vez que cambia la lista de mapas
  $effect(() => {
    for (const m of partida.mapas) miniaturas.asegurar(m.url_imagen_fondo);
  });

  // ---- Imágenes ----
  async function asignarImagen(mapa: Mapa, rutaOrigen: string) {
    if (mapa.url_imagen_fondo?.trim()) {
      reemplazo = { mapa, rutaOrigen };
      return;
    }
    await partida.cambiarImagenMapa(mapa, rutaOrigen);
  }

  async function confirmarReemplazo() {
    if (!reemplazo) return;
    const { mapa, rutaOrigen } = reemplazo;
    reemplazo = null;
    await partida.cambiarImagenMapa(mapa, rutaOrigen);
  }

  async function abrirSelector(mapa: Mapa) {
    menu = null;
    const ruta = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Imágenes", extensions: EXTENSIONES_IMAGEN.mapa }]
    });
    if (typeof ruta === "string") await asignarImagen(mapa, ruta);
  }

  // Soltar una imagen del sistema: sobre una tarjeta la asigna a ese mapa, sobre la lista crea uno nuevo
  onMount(() =>
    alSoltarArchivos(async ([ruta], el) => {
      if (!esImagen(ruta, "mapa")) return;
      const tarjeta = el?.closest<HTMLElement>(".tarjeta-mapa");
      if (tarjeta?.dataset.mapaId) {
        const mapa = partida.mapas.find((m) => m.id === Number(tarjeta.dataset.mapaId));
        if (mapa) return asignarImagen(mapa, ruta);
      }
      if (el?.closest(".sub-mapas")) await partida.crearMapa(ruta);
    })
  );

  // ---- Nombre ----
  function iniciarEdicion(m: Mapa) {
    menu = null;
    editandoId = m.id;
    textoEdicion = m.nombre;
  }

  async function guardarEdicion(m: Mapa) {
    if (editandoId === null) return;
    editandoId = null;
    const nombre = textoEdicion.trim() || m.nombre;
    if (nombre !== m.nombre) await partida.renombrarMapa(m, nombre);
  }

  function abrirMenu(e: MouseEvent, mapa: Mapa) {
    e.preventDefault();
    menu = { x: e.clientX, y: e.clientY, mapa };
  }
</script>

<div class="lista sub-mapas">
  <span class="titulo-mini">MAPAS</span>
  <div class="scroll">
    {#each lista.visuales as m, i (m.id)}
      <div animate:flip={{ duration: 150 }}>
        <button
          type="button"
          class="tarjeta-mapa"
          class:seleccionado={partida.mapaActual?.id === m.id}
          class:fantasma-origen={lista.activo && lista.item?.id === m.id}
          data-mapa-id={m.id}
          data-indice={i}
          onpointerdown={(e) => lista.iniciar(m, i, e)}
          ondblclick={(e) => {
            e.stopPropagation();
            iniciarEdicion(m);
          }}
          oncontextmenu={(e) => abrirMenu(e, m)}
        >
          <!-- Contenedor de imagen blindado para no interceptar clics/arrastres -->
          {#if miniaturas.de(m.url_imagen_fondo)}
            <div class="contenedor-low">
              <img src={miniaturas.de(m.url_imagen_fondo)} alt="" class="img-low" draggable="false" />
            </div>
          {:else}
            <span class="previa-mapa">🗺️</span>
          {/if}

          {#if editandoId === m.id}
            <input
              type="text"
              class="input-nombre"
              bind:value={textoEdicion}
              onblur={() => guardarEdicion(m)}
              onkeydown={(e) => {
                if (e.key === "Enter") guardarEdicion(m);
                if (e.key === "Escape") editandoId = null;
              }}
              onclick={(e) => e.stopPropagation()}
              autofocus
            />
          {:else}
            <span class="nombre-mapa texto-recortado">{m.nombre}</span>
          {/if}
          {#if sala.mapaPublico === m.id}
            <span class="publico" title="Es el mapa que ven los jugadores">👁</span>
          {/if}
        </button>
      </div>
    {/each}
    <button
      class="tarjeta-mapa nueva"
      disabled={partida.zonaId === null}
      onclick={() => partida.crearMapa(null)}
      title={partida.zonaId === null ? "Crea primero una zona" : "Añadir mapa"}
    >+</button>
  </div>
</div>

<ArrastreFlotante {lista}>
  {#if lista.item}
    <div class="tarjeta-mapa clon seleccionado">
      {#if miniaturas.de(lista.item.url_imagen_fondo)}
        <div class="contenedor-low">
          <img src={miniaturas.de(lista.item.url_imagen_fondo)} alt="" class="img-low" />
        </div>
      {:else}
        <span class="previa-mapa">🗺️</span>
      {/if}
      <span class="nombre-mapa texto-recortado">{lista.item.nombre}</span>
    </div>
  {/if}
</ArrastreFlotante>

{#if menu}
  {@const mapa = menu.mapa}
  <MenuContextual
    x={menu.x}
    y={menu.y}
    onCerrar={() => (menu = null)}
    opciones={[
      { texto: "Cambiar imagen", accion: () => abrirSelector(mapa) },
      { texto: "Cambiar nombre", accion: () => iniciarEdicion(mapa) },
      { texto: "Borrar mapa", peligro: true, accion: () => partida.borrarMapa(mapa) }
    ]}
  />
{/if}

{#if reemplazo}
  <VentanaReemplazoImagen mapa={reemplazo.mapa} onConfirmar={confirmarReemplazo} onCancelar={() => (reemplazo = null)} />
{/if}

<style>
  .lista { flex: 1; min-height: 0; display: flex; flex-direction: column; }
  .titulo-mini { font-size: 10px; font-weight: bold; color: #71717a; margin-bottom: 3px; display: block; }
  .scroll { flex: 1; overflow-y: auto; display: flex; flex-direction: column; gap: 3px; }

  .tarjeta-mapa {
    background: #1f1f23; border: 1px solid #3f3f46; border-radius: 4px; padding: 3px;
    text-align: center; cursor: grab; width: 100%; color: inherit; font: inherit; display: block;
    touch-action: none;
  }
  .tarjeta-mapa:active { cursor: grabbing; }
  .tarjeta-mapa.seleccionado { border-color: #3b82f6; background: #2a2a32; }
  .tarjeta-mapa.nueva { border-style: dashed; font-size: 14px; padding: 6px; cursor: pointer; }
  .tarjeta-mapa.nueva:disabled { opacity: 0.4; cursor: not-allowed; }
  .previa-mapa { font-size: 16px; display: block; pointer-events: none; }
  .tarjeta-mapa { position: relative; }
  .publico { position: absolute; top: 2px; right: 3px; font-size: 10px; pointer-events: none; }
  .nombre-mapa { font-size: 9px; color: #d4d4d8; display: block; margin-top: 2px; pointer-events: none; }

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

  /* Blindaje total para que la imagen no robe el evento de arrastre */
  .contenedor-low {
    width: 100%; height: 36px; border-radius: 2px; overflow: hidden;
    background: #09090b; display: flex; align-items: center; justify-content: center;
    pointer-events: none;
  }
  .img-low {
    width: 100%; height: 100%; object-fit: cover;
    pointer-events: none; -webkit-user-drag: none; user-select: none;
  }

  .clon { cursor: grabbing !important; }
</style>
