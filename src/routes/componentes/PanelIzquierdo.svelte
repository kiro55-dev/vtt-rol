<!-- src/routes/componentes/PanelIzquierdo.svelte -->
<!-- Solo la estructura del panel: cada parte vive en su componente y lee el estado de `partida`. -->
<script lang="ts">
  import { partida } from "$lib/partida.svelte";
  import ListaZonas from "./panel-izquierdo/ListaZonas.svelte";
  import ListaMapas from "./panel-izquierdo/ListaMapas.svelte";
  import BancoEntidades from "./panel-izquierdo/BancoEntidades.svelte";
  import { bancoExterno } from "$lib/bancoExterno.svelte"; // BANCO EXTERNO
  import BotonAjustes from "./panel-izquierdo/BotonAjustes.svelte";
  import VentanaParametros from "./VentanaParametros.svelte";
  import VentanaPartidas from "./VentanaPartidas.svelte";

  let parametrosAbierto = $state(false);
  let partidasAbierto = $state(false);

  // Más opciones de configuración se añaden aquí
  const opciones = $derived([
    {
      texto: "Partidas…",
      accion: () => (partidasAbierto = true),
      // Una ficha nueva abierta tiene su carpeta en esta partida: no puede acompañarte a otra
      deshabilitada: partida.entidadEnEdicion ? "Cierra la ficha que tienes abierta" : undefined
    },
    {
      texto: "Editar parámetros de esta partida",
      accion: () => (parametrosAbierto = true),
      // Una ficha abierta ya copió los parámetros: se cambian cuando está cerrada
      deshabilitada: partida.entidadEnEdicion ? "Cierra la ficha que tienes abierta" : undefined
    }
  ]);
</script>

<aside class="panel-izquierdo">
  <div class="bloque-arriba">
    <div class="subseccion"><ListaZonas /></div>
    <div class="subseccion"><ListaMapas /></div>
  </div>

  <div class="bloque-abajo">
    {#if bancoExterno.abierto}
      <!-- BANCO EXTERNO: el banco está en otra ventana. Soltar aquí una instancia la elimina, como en el banco -->
      <div class="banco-fuera" data-destino="banco">
        <span>Banco de fichas en otra ventana</span>
        <button type="button" onclick={() => void bancoExterno.cerrar()}>Traerlo de vuelta</button>
      </div>
    {:else}
      <BancoEntidades />
    {/if}
  </div>

  <div class="pie">
    <BotonAjustes {opciones} />
  </div>
</aside>

{#if parametrosAbierto}
  <VentanaParametros onCerrar={() => (parametrosAbierto = false)} />
{/if}

{#if partidasAbierto}
  <VentanaPartidas onCerrar={() => (partidasAbierto = false)} />
{/if}

<style>
  .panel-izquierdo {
    width: clamp(180px, 14vw, 225px);
    height: 100%;
    background: #141416;
    border-right: 1px solid #27272a;
    display: flex;
    flex-direction: column;
    box-sizing: border-box;
    padding: 6px;
    gap: 6px;
    user-select: none;
    font-size: 11px;
  }
  .bloque-arriba { height: 44%; display: flex; gap: 4px; border-bottom: 1px solid #27272a; padding-bottom: 4px; }
  .subseccion { flex: 1; display: flex; flex-direction: column; overflow: hidden; }
  .bloque-abajo { flex: 1; display: flex; flex-direction: column; overflow: hidden; gap: 5px; }
  .banco-fuera {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    border: 1px dashed #3f3f46;
    border-radius: 6px;
    color: #71717a;
    text-align: center;
  }
  .banco-fuera button {
    background: #27272a;
    border: 1px solid #3f3f46;
    color: #a1a1aa;
    border-radius: 3px;
    font-size: 10px;
    padding: 3px 8px;
    cursor: pointer;
  }
  .banco-fuera button:hover { background: #3f3f46; color: white; }
  .pie { display: flex; justify-content: flex-start; border-top: 1px solid #27272a; padding-top: 4px; }
</style>
