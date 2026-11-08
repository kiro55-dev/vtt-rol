<!-- src/routes/+page.svelte -->
<script lang="ts">
  import { onMount } from "svelte";
  import { partida } from "$lib/partida.svelte";
  import { tiradas } from "$lib/tiradas.svelte";
  import { alRecibirAviso } from "$lib/avisosTablero";
  import { bancoExterno } from "$lib/bancoExterno.svelte"; // BANCO EXTERNO

  import PanelIzquierdo from "./componentes/PanelIzquierdo.svelte";
  import PanelDerecho from "./componentes/PanelDerecho.svelte";
  import Tablero from "./componentes/Tablero.svelte";
  import BarraRejilla from "./componentes/BarraRejilla.svelte";
  import BarraDados from "./componentes/BarraDados.svelte";
  import VentanaFicha from "./componentes/VentanaFicha.svelte";
  import FantasmaFicha from "./componentes/comunes/FantasmaFicha.svelte";
  import VisorImagen from "./componentes/comunes/VisorImagen.svelte";
  let fichaParaEditar = $state<any>(null); // por ahora nadie abre el modal; se tipará con Entidad/Instancia

  // Un clic izquierdo en cualquier parte quita el foco de las fichas, salvo en lo que lo gestiona
  // por su cuenta (tablero, «Fichas activas», parámetros, menús): lo marcan con data-conserva-foco.
  function alPulsarEnLaVentana(e: PointerEvent) {
    if (e.button !== 0) return;
    const objetivo = e.target as Element | null;
    // La selección del banco (Ctrl+clic) se quita al pulsar fuera del banco
    if (!objetivo?.closest?.(".banco")) partida.limpiarSeleccionBanco();
    if (partida.idsSeleccionados.length === 0) return;
    if (objetivo?.closest?.("[data-conserva-foco]")) return;
    partida.seleccionarInstancia(null);
  }

  // Supr: borra las fichas seleccionadas del banco (a la papelera) o, si no hay, las instancias con foco.
  // Ctrl+Z: restaura lo último que se borró de la papelera. ↑ / ↓: con una sola ficha con foco, pasa a la
  // anterior / siguiente de «Fichas activas».
  function alPulsarTecla(e: KeyboardEvent) {
    const t = e.target as HTMLElement | null;
    // Dentro de un campo de texto las teclas son del campo (Supr borra letras, Ctrl+Z deshace lo escrito)
    if (t?.closest?.("input, textarea, select, [contenteditable='true']")) return;
    if (e.key === "Escape") partida.limpiarSeleccionBanco();
    if ((e.ctrlKey || e.metaKey) && !e.shiftKey && e.key.toLowerCase() === "z") {
      e.preventDefault();
      void partida.restaurarPapelera();
      return;
    }
    if (e.key === "Delete") {
      const delBanco = [...partida.idsBancoSeleccionados];
      if (delBanco.length > 0) {
        e.preventDefault();
        void partida.borrarEntidades(delBanco);
        return;
      }
      // Instancias con foco: igual que arrastrarlas al banco (no van a la papelera)
      const ids = [...partida.idsSeleccionados];
      if (ids.length === 0) return;
      e.preventDefault();
      void (async () => {
        for (const id of ids) await partida.borrarInstancia(id);
      })();
      return;
    }
    if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
    if (t?.closest?.("input, textarea, select, [contenteditable='true']")) return;
    if (partida.seleccionarVecina(e.key === "ArrowUp" ? -1 : 1)) {
      e.preventDefault();
      // El botón de la tarjeta pulsada conserva el foco del navegador (y su borde): se suelta
      (document.activeElement as HTMLElement | null)?.blur?.();
    }
  }

  // El foco de otro jugador (aviso por red, no se guarda) se dibuja aparte del propio
  onMount(() =>
    alRecibirAviso((aviso) => {
      if ("Foco" in aviso) partida.recibirFocoAjeno(aviso.Foco.ids_instancias);
    })
  );

  // BANCO EXTERNO: atiende a la ventana del banco y le avisa cuando cambian las fichas, las instancias o la zona
  onMount(() => bancoExterno.escuchar());
  $effect(() => {
    void [partida.banco, partida.instancias, partida.zonaId, bancoExterno.abierto];
    void bancoExterno.avisarCambio();
  });

  onMount(async () => {
    try {
      await partida.iniciar();
      await tiradas.cargar();
    } catch (err) {
      console.error("Error al obtener datos de Rust:", err);
    }
  });
</script>

<svelte:window onpointerdown={alPulsarEnLaVentana} onkeydown={alPulsarTecla} />

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

  <!-- La ficha que se arrastra entre el banco, el tablero y «Fichas activas» -->
  <FantasmaFicha />

  <!-- La foto de una ficha activa a resolución completa, en una ventanita junto al puntero (doble clic en su tarjeta) -->
  <VisorImagen />
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