<!-- src/routes/+page.svelte -->
<script lang="ts">
  import { onMount } from "svelte";
  import { partida } from "$lib/partida.svelte";
  import { tiradas } from "$lib/tiradas.svelte";

  import PanelIzquierdo from "./componentes/PanelIzquierdo.svelte";
  import PanelDerecho from "./componentes/PanelDerecho.svelte";
  import Tablero from "./componentes/Tablero.svelte";
  import BarraRejilla from "./componentes/BarraRejilla.svelte";
  import BarraDados from "./componentes/BarraDados.svelte";
  import VentanaFicha from "./componentes/VentanaFicha.svelte";
  let fichaParaEditar = $state<any>(null); // por ahora nadie abre el modal; se tipará con Entidad/Instancia

  onMount(async () => {
    try {
      await partida.iniciar();
      await tiradas.cargar();
    } catch (err) {
      console.error("Error al obtener datos de Rust:", err);
    }
  });
</script>

<div class="contenedor-principal">
  <PanelIzquierdo />

  <main class="area-central">
    <div class="barra-superior">
      <BarraRejilla />
    </div>

    <div class="contenedor-lienzo">
      <Tablero mapa={partida.mapaActual} rejilla={partida.rejilla} />
    </div>

    <div class="barra-inferior">
      <BarraDados
        onSumarAlChat={(fragmento: string) => tiradas.sumarDado(fragmento)}
        onRestarDelChat={(fragmento: string) => tiradas.restarDado(fragmento)}
        onLanzarTirada={(bonificador: number | null) => tiradas.tirarConBonificador(bonificador)}
      />
    </div>
  </main>

  <PanelDerecho />

  <!-- Ventana flotante no bloqueante -->
  {#if partida.entidadEnEdicion}
    <VentanaFicha entidad={partida.entidadEnEdicion} />
  {/if}
</div>

<style>
  :global(body, html) {
    margin: 0; padding: 0; width: 100vw; height: 100vh;
    overflow: hidden; background: #09090b; color: #fafafa;
    font-family: -apple-system, BlinkMacSystemFont, sans-serif;
  }
  .contenedor-principal { display: flex; width: 100vw; height: 100vh; }
  .area-central { flex: 1; position: relative; display: flex; flex-direction: column; overflow: hidden; }
  .contenedor-lienzo { flex: 1; width: 100%; height: 100%; }
  .barra-superior { position: absolute; top: 8px; left: 50%; transform: translateX(-50%); z-index: 10; }
  .barra-inferior { position: absolute; bottom: 8px; left: 50%; transform: translateX(-50%); z-index: 10; }

  /* Scrollbar ultrafina global */
  :global(*) {
    scrollbar-width: thin;
    scrollbar-color: #3f3f46 transparent;
  }
  :global(*::-webkit-scrollbar) {
    width: 4px;
    height: 4px;
  }
  :global(*::-webkit-scrollbar-track) {
    background: transparent;
  }
  :global(*::-webkit-scrollbar-thumb) {
    background: #3f3f46;
    border-radius: 4px;
  }
  :global(*::-webkit-scrollbar-thumb:hover) {
    background: #71717a;
  }
</style>