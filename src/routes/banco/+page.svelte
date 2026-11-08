<!-- src/routes/banco/+page.svelte -->
<!-- BANCO EXTERNO: el banco de fichas en su propia ventana (ver $lib/bancoExterno). Es otro contexto de
     JavaScript: lee sus datos de Rust y se mantiene al día con eventos de la ventana principal. -->
<script lang="ts">
  import { onMount } from "svelte";
  import { listen, emit } from "@tauri-apps/api/event";
  import { partida } from "$lib/partida.svelte";
  import { EVENTO_CAMBIADO, EVENTO_PEDIR_ESTADO } from "$lib/bancoExterno.svelte";
  import BancoEntidades from "../componentes/panel-izquierdo/BancoEntidades.svelte";

  onMount(() => {
    let quitar: (() => void) | null = null;
    let terminado = false;

    (async () => {
      // Se escucha ANTES de pedir el estado, para no perder la respuesta
      const fn = await listen<{ zonaId: number | null }>(EVENTO_CAMBIADO, async (e) => {
        partida.zonaId = e.payload.zonaId; // el filtro «Zona actual» sigue a la ventana principal
        await partida.recargarEntidades();
      });
      if (terminado) return fn();
      quitar = fn;
      await partida.iniciar();
      await emit(EVENTO_PEDIR_ESTADO);
    })().catch((err) => console.error("Banco externo:", err));

    return () => {
      terminado = true;
      quitar?.();
    };
  });
</script>

<div class="ventana-banco">
  <BancoEntidades externo />
</div>

<style>
  :global(body, html) {
    margin: 0; padding: 0; width: 100vw; height: 100vh;
    overflow: hidden; background: #141416; color: #fafafa;
    font-family: -apple-system, BlinkMacSystemFont, sans-serif;
  }
  :global(*) { scrollbar-width: thin; scrollbar-color: #3f3f46 transparent; }
  .ventana-banco {
    width: 100vw;
    height: 100vh;
    box-sizing: border-box;
    padding: 8px;
    display: flex;
    flex-direction: column;
    font-size: 11px;
    user-select: none;
  }
</style>
