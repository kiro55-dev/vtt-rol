<!-- src/routes/componentes/Notificaciones.svelte -->
<!-- Avisos pasajeros (p. ej. «Ana se unió a la sala») en la esquina superior izquierda del TABLERO, uno debajo de otro por
     orden de llegada. Cada uno se quita solo a su hora (ver $lib/notificaciones). No capturan el ratón: se puede seguir
     dibujando o moviendo fichas por debajo. Se montan dentro del área del tablero (`position: relative`). -->
<script lang="ts">
  import { flip } from "svelte/animate";
  import { fade, fly } from "svelte/transition";
  import { notificaciones } from "$lib/notificaciones.svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";

  function abrir(e: MouseEvent, url: string) {
    e.preventDefault();
    void openUrl(url).catch(() => {});
  }
</script>

<div class="notificaciones" role="status" aria-live="polite">
  {#each notificaciones.lista as n (n.id)}
    <div class="aviso" class:largo={n.largo} class:atencion={n.aviso} class:cerrable={n.cerrable} animate:flip={{ duration: 150 }} in:fly={{ x: -16, duration: 150 }} out:fade={{ duration: 150 }}>{#if n.cerrable}<button type="button" class="cerrar" aria-label="Cerrar aviso" onclick={() => notificaciones.quitar(n.id)}>×</button>{/if}{n.texto}{#if n.enlace}<a class="enlace" href={n.enlace.url} onclick={(e) => abrir(e, n.enlace!.url)}>{n.enlace.texto}</a>{/if}</div>
  {/each}
</div>

<style>
  .notificaciones {
    position: absolute; top: 8px; left: 8px; z-index: 20;
    display: flex; flex-direction: column; align-items: flex-start; gap: 6px;
    max-width: min(320px, 40%); pointer-events: none;
  }
  .aviso.largo { white-space: normal; text-overflow: clip; line-height: 1.35; }
  .aviso .enlace { display: block; margin-top: 4px; color: #60a5fa; text-decoration: underline; cursor: pointer; pointer-events: auto; }
  .aviso.cerrable { position: relative; padding-right: 26px; }
  .aviso .cerrar {
    position: absolute; top: 2px; right: 4px; width: 18px; height: 18px; padding: 0; line-height: 16px;
    background: none; border: none; border-radius: 4px; color: #a1a1aa; font-size: 16px; cursor: pointer; pointer-events: auto;
  }
  .aviso .cerrar:hover { color: #f4f4f5; background: #3f3f46; }
  .aviso.atencion { border-left-color: #f59e0b; }
  .aviso {
    max-width: 100%; box-sizing: border-box; padding: 6px 12px;
    background: rgba(24, 24, 27, 0.92); border: 1px solid #3f3f46; border-left: 3px solid #22c55e; border-radius: 6px;
    box-shadow: 0 6px 18px rgba(0, 0, 0, 0.5);
    color: #f4f4f5; font-size: 12px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
</style>
