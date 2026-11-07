<!-- src/routes/componentes/panel-izquierdo/BotonAjustes.svelte -->
<!-- Rueda de ajustes de la esquina inferior izquierda: abre un menú con las opciones que le pasen,
     así se pueden añadir más sin tocar este componente. -->
<script lang="ts">
  interface Opcion {
    texto: string;
    accion: () => void;
    /** Motivo por el que ahora no se puede usar (se muestra al pasar el ratón) */
    deshabilitada?: string;
  }

  let { opciones }: { opciones: Opcion[] } = $props();
  let abierto = $state(false);

  function elegir(o: Opcion) {
    if (o.deshabilitada) return;
    abierto = false;
    o.accion();
  }
</script>

<!-- Un clic en cualquier otro sitio cierra el menú (el del propio botón no llega: stopPropagation) -->
<svelte:window onclick={() => (abierto = false)} onkeydown={(e) => e.key === "Escape" && (abierto = false)} />

<div class="ajustes">
  {#if abierto}
    <div class="menu" role="menu">
      {#each opciones as o}
        <button type="button" role="menuitem" class:desactivada={!!o.deshabilitada} title={o.deshabilitada} onclick={(e) => { e.stopPropagation(); elegir(o); }}>
          {o.texto}
        </button>
      {/each}
    </div>
  {/if}

  <button
    type="button"
    class="btn-ajustes"
    class:abierto
    title="Configuración"
    aria-label="Configuración"
    aria-haspopup="menu"
    aria-expanded={abierto}
    onclick={(e) => {
      e.stopPropagation();
      abierto = !abierto;
    }}>⚙</button
  >
</div>

<style>
  .ajustes { position: relative; }
  .btn-ajustes {
    background: none; border: none; color: #71717a; cursor: pointer; font-size: 16px;
    padding: 2px 6px; border-radius: 4px; line-height: 1;
  }
  .btn-ajustes:hover, .btn-ajustes.abierto { color: white; background: #27272a; }

  .menu {
    position: absolute; bottom: 100%; left: 0; margin-bottom: 4px; min-width: 190px; z-index: 900;
    background: #1c1c1f; border: 1px solid #3f3f46; border-radius: 4px; padding: 3px;
    display: flex; flex-direction: column; gap: 2px; box-shadow: 0 4px 12px rgba(0, 0, 0, 0.5);
  }
  .menu button {
    background: none; border: none; color: #f4f4f5; font-size: 11px; padding: 5px 8px;
    text-align: left; cursor: pointer; border-radius: 2px;
  }
  .menu button:hover { background: #27272a; }
  .menu button.desactivada { color: #52525b; cursor: not-allowed; }
  .menu button.desactivada:hover { background: none; }
</style>
