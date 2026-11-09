<!-- cliente-web/src/App.svelte -->
<!-- Página del jugador: lee el enlace (#t&s&k), se conecta al host y enseña la entrada o el tablero. -->
<script lang="ts">
  import { onMount } from "svelte";
  import { leerEnlace } from "./enlace";
  import { Conexion } from "./conexion.svelte";
  import Entrada from "./Entrada.svelte";
  import TableroJugador from "./TableroJugador.svelte";
  import ListaFichas from "./ListaFichas.svelte";

  const enlace = leerEnlace(location.hash);
  const conexion = enlace ? new Conexion(enlace) : null;

  onMount(() => {
    conexion?.conectar();
    return () => conexion?.cerrar();
  });
</script>

<main>
  {#if !conexion}
    <div class="aviso">
      <h1>OpenRoll</h1>
      <p>Este enlace no es válido. Pide al host el enlace de la sala.</p>
    </div>
  {:else if conexion.estado.tipo === "conectando"}
    <div class="aviso"><p>Conectando con la sala…</p></div>
  {:else if conexion.estado.tipo === "identificate"}
    <Entrada {conexion} estado={conexion.estado} />
  {:else if conexion.estado.tipo === "fuera"}
    <div class="aviso">
      <h1>Fuera de la sala</h1>
      <p>{conexion.estado.motivo}</p>
      <button type="button" onclick={() => location.reload()}>Volver a intentarlo</button>
    </div>
  {:else}
    <div class="juego">
      <div class="tablero"><TableroJugador {conexion} /></div>
      <aside class="lateral">
        <div class="yo">
          <span>Conectado como <b>{conexion.yo?.nombre}</b></span>
          {#if conexion.estado.tipo === "reconectando"}<span class="reconectando">· reconectando…</span>{/if}
        </div>
        <ListaFichas {conexion} />
      </aside>
    </div>
  {/if}
</main>

<style>
  :global(html, body) {
    margin: 0; padding: 0; height: 100%; background: #09090b; color: #fafafa;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; overflow: hidden;
  }
  :global(#app) { height: 100%; }
  main { height: 100%; }
  .aviso { height: 100%; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 8px; padding: 16px; text-align: center; }
  .aviso h1 { margin: 0; font-size: 20px; }
  .aviso p { margin: 0; color: #a1a1aa; font-size: 14px; }
  .aviso button { background: #2563eb; color: white; border: none; border-radius: 4px; padding: 6px 12px; cursor: pointer; }
  .juego { display: flex; height: 100%; }
  .tablero { flex: 1; min-width: 0; position: relative; }
  .lateral { width: 240px; background: #141416; border-left: 1px solid #27272a; display: flex; flex-direction: column; font-size: 12px; }
  .yo { padding: 8px; border-bottom: 1px solid #27272a; color: #a1a1aa; }
  .yo b { color: #fafafa; }
  .reconectando { color: #facc15; }
  @media (max-width: 640px) {
    .juego { flex-direction: column; }
    .lateral { width: auto; height: 35%; border-left: none; border-top: 1px solid #27272a; }
  }
</style>
