<!-- src/routes/+page.svelte -->
<script lang="ts">
  import { onMount } from "svelte";
  import { partida } from "$lib/partida.svelte";
  import { tiradas } from "$lib/tiradas.svelte";
  import { ajustesVista } from "$lib/ajustesVista.svelte";
  import { alRecibirAviso } from "$lib/avisosTablero";
  import { sala } from "$lib/sala.svelte";
  import { esJugador } from "$lib/modo";
  import { conexion } from "$lib/red/conexion.svelte";
  import EntradaSala from "./componentes/EntradaSala.svelte";

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
    // Ctrl+Z (la papelera) es del host; el jugador solo borra sus instancias con Supr (más abajo)
    if (esJugador && (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "z") return;
    if ((e.ctrlKey || e.metaKey) && !e.shiftKey && e.key.toLowerCase() === "z") {
      e.preventDefault();
      void partida.restaurarPapelera();
      return;
    }
    if (e.key === "Delete") {
      const delBanco = esJugador ? [] : [...partida.idsBancoSeleccionados]; // borrar fichas del banco es del host
      if (delBanco.length > 0) {
        e.preventDefault();
        void partida.borrarEntidades(delBanco);
        return;
      }
      // Instancias con foco: igual que arrastrarlas al banco (no van a la papelera)
      const ids = partida.idsSeleccionados.filter((id) => {
        const i = partida.instancias.find((x) => x.id_instancia === id);
        return !!i && partida.controla(i); // el jugador, solo las suyas
      });
      if (ids.length === 0) return;
      e.preventDefault();
      void partida.borrarInstancias(ids);
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

  onMount(async () => {
    try {
      if (esJugador) {
        // Jugador: los datos llegan del host. La app empieza a leer cuando llega su primera vista.
        if (!conexion) return;
        conexion.conectar();
        await conexion.lista;
        await partida.iniciar();
        return;
      }
      await partida.iniciar();
      await tiradas.cargar();
      await sala.iniciar();
    } catch (err) {
      console.error("Error al obtener los datos de la partida:", err);
    }
  });

  // Jugador: la app se enseña en cuanto está dentro (y se mantiene mientras reconecta)
  const dentro = $derived(!esJugador || conexion?.estado.tipo === "dentro" || conexion?.estado.tipo === "reconectando");
</script>

<svelte:window onpointerdown={alPulsarEnLaVentana} onkeydown={alPulsarTecla} />

{#if esJugador && !dentro}
  <div class="aviso-jugador">
    {#if !conexion}
      <h1>OpenRoll</h1>
      <p>Este enlace no es válido. Pide al host el enlace de la sala.</p>
    {:else if conexion.estado.tipo === "identificate"}
      <EntradaSala {conexion} estado={conexion.estado} />
    {:else if conexion.estado.tipo === "fuera"}
      <h1>Fuera de la sala</h1>
      <p>{conexion.estado.motivo}</p>
      <button type="button" onclick={() => location.reload()}>Volver a intentarlo</button>
    {:else}
      <p>Conectando con la sala…</p>
    {/if}
  </div>
{:else}
<div class="contenedor-principal">
  <PanelIzquierdo />

  <main class="area-central">
    {#if ajustesVista.edicionRejilla && !esJugador}
      <!-- Se oculta desde ⚙ (preferencia de esta pantalla, no de la partida) -->
      <div class="barra-superior">
        <BarraRejilla />
      </div>
    {/if}

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
    {#if partida.avisos}
      <div class="aviso-linea" role="status">{partida.avisos}</div>
    {/if}
    {#if esJugador && conexion?.estado.tipo === "reconectando"}
      <div class="yo-jugador">
        {#if conexion.estado.tipo === "reconectando"}<span class="reconectando">Reconectando…</span>{/if}
      </div>
    {/if}
  </main>

  <PanelDerecho />

  <!-- Ventana flotante no bloqueante -->
  {#if partida.entidadEnEdicion}
    <!-- Una ventana nueva por cada ficha abierta, también al pasar de una a otra sin cerrar -->
    {#key partida.sesionEditor}
      <VentanaFicha entidad={partida.entidadEnEdicion} />
    {/key}
  {/if}

  <!-- La ficha que se arrastra entre el banco, el tablero y «Fichas activas» -->
  <FantasmaFicha />

  <!-- La foto de una ficha activa a resolución completa, en una ventanita junto al puntero (doble clic en su tarjeta) -->
  <VisorImagen />
</div>
{/if}

<style>
  .aviso-linea {
    position: absolute; left: 50%; bottom: 44px; transform: translateX(-50%); z-index: 900;
    background: #7f1d1d; color: #fecaca; border: 1px solid #b91c1c; border-radius: 6px;
    padding: 4px 10px; font-size: 11px; max-width: 80%;
  }
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
  .yo-jugador {
    position: absolute; bottom: 8px; left: 50%; transform: translateX(-50%); z-index: 10;
    background: rgba(24, 24, 27, 0.85); border: 1px solid #3f3f46; border-radius: 4px; padding: 3px 10px;
    font-size: 11px; color: #a1a1aa;
  }
  .reconectando { color: #facc15; }
  .aviso-jugador {
    height: 100vh; display: flex; flex-direction: column; align-items: center; justify-content: center;
    gap: 8px; padding: 16px; text-align: center; box-sizing: border-box;
  }
  .aviso-jugador h1 { margin: 0; font-size: 20px; }
  .aviso-jugador p { margin: 0; color: #a1a1aa; font-size: 14px; }
  .aviso-jugador button { background: #2563eb; color: white; border: none; border-radius: 4px; padding: 6px 12px; cursor: pointer; }

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