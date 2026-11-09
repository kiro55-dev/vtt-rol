<!-- src/routes/componentes/panel-derecho/TiradasChat.svelte -->
<!-- Tiradas y chat integrados en el mismo recuadro: la lista arriba y la caja de comandos abajo. -->
<script>
  import { ayuda } from "$lib/ajustesVista.svelte";
  import ListaTiradas from "./ListaTiradas.svelte";
  import EntradaComando from "./EntradaComando.svelte";
  import { tiradas } from "$lib/tiradas.svelte";
  import { esJugador } from "$lib/modo";
</script>

<div class="cabecera">
  <span class="titulo-mini">TIRADAS</span>
  {#if !esJugador}
    <!-- Solo el host: en privado, las tiradas que haga no llegan a los jugadores -->
    <button
      type="button"
      class="filtro"
      class:activo={!tiradas.publica}
      aria-pressed={!tiradas.publica}
      data-conserva-foco
      title={ayuda(tiradas.publica
        ? "Los jugadores ven tus tiradas. Pulsa para tirar en privado"
        : "Solo tú ves tus tiradas. Pulsa para que las vean los jugadores")}
      onclick={() => (tiradas.publica = !tiradas.publica)}
    >
      {tiradas.publica ? "Tirada pública" : "Tirada privada"}
    </button>
  {/if}
</div>
<div class="recuadro-chat-tiradas">
  <ListaTiradas />
  <EntradaComando />
</div>

<style>
  .cabecera { display: flex; justify-content: space-between; align-items: center; margin-bottom: 3px; }
  .titulo-mini { font-size: 10px; font-weight: bold; color: #71717a; display: block; }
  .filtro {
    background: #1c1c1f;
    border: 1px solid #27272a;
    color: #a1a1aa;
    font-size: 10px;
    padding: 1px 6px;
    border-radius: 3px;
    cursor: pointer;
    line-height: normal;
  }
  .filtro:hover { background: #27272a; color: #f4f4f5; }
  .filtro.activo { background: #27272a; color: #ffffff; border-color: #3f3f46; font-weight: bold; }
  .recuadro-chat-tiradas {
    flex: 1;
    background: #09090b;
    border: 1px solid #27272a;
    border-radius: 6px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
</style>
