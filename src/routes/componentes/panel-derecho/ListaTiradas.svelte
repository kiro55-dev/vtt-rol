<!-- src/routes/componentes/panel-derecho/ListaTiradas.svelte -->
<script lang="ts">
  import { tiradas, desgloseSeguro } from "$lib/tiradas.svelte";
  import { arrastreTirada } from "$lib/arrastreTirada.svelte";
</script>

<!-- Una tirada válida se puede arrastrar sobre el valor de una ficha: botón izquierdo = quita el total,
     derecho = lo suma. `data-conserva-foco`: empezar a arrastrar no debe quitar el foco a la ficha. -->
<div class="lista-tiradas" role="list" data-conserva-foco oncontextmenu={(e) => e.preventDefault()}>
  {#each tiradas.historial as t}
    <div
      class="linea-tirada"
      class:error={t.total === null}
      class:arrastrable={t.total !== null}
      role="listitem"
      title={t.total === null ? undefined : "Arrastra sobre un valor de una ficha: botón izquierdo quita, derecho suma"}
      onpointerdown={(e) => t.total !== null && arrastreTirada.preparar(e, t.total)}
    >
      {#if t.total !== null}
        <strong class="total">= {t.total}</strong>
      {/if}
      {#if t.total !== null}<span class="hora">{t.autor || "rollmaster"}{t.publica === false ? " · p" : ""}</span>{/if}
      <!-- @html a propósito: Rust devuelve el desglose con <b> para resaltar el dado ganador. Lo genera
           solo Rust con números y palabras fijas (el comando escrito nunca llega aquí), y además
           `desgloseSeguro` escapa todo salvo <b> y </b>. -->
      <span class="cmd">{@html desgloseSeguro(t.comando)}</span>
    </div>
  {/each}
</div>

<style>
  .lista-tiradas {
    flex: 1;
    overflow-y: auto;
    padding: 6px;
    font-family: monospace;
    display: flex;
    flex-direction: column;
    gap: 4px;

    /* SCROLLBAR ULTRAFINA (Firefox y navegadores modernos) */
    scrollbar-width: thin;
    scrollbar-color: #3f3f46 transparent;
  }
  /* SCROLLBAR ULTRAFINA (Chrome, Edge, Tauri WebView) */
  .lista-tiradas::-webkit-scrollbar { width: 3px; }
  .lista-tiradas::-webkit-scrollbar-track { background: transparent; }
  .lista-tiradas::-webkit-scrollbar-thumb { background: #3f3f46; border-radius: 4px; }
  .lista-tiradas::-webkit-scrollbar-thumb:hover { background: #71717a; }

  .linea-tirada.arrastrable { cursor: grab; user-select: none; }
  /* Sangría fija: la primera línea arranca en el borde y las demás a 4 caracteres, sea cual sea el autor.
     El total flota a la derecha de la primera línea (por eso va antes en el marcado). */
  .linea-tirada {
    display: block;
    padding: 1px 0 1px 4ch;
    text-indent: -4ch;
  }

  .hora {
    text-indent: 0; /* el inline-block heredaría la sangría negativa y se cortaría por la izquierda */
    color: #52525b;
    font-size: 9px;
    display: inline-block;
    margin-right: 6px;
    letter-spacing: -0.5px;
    user-select: none;
  }

  .cmd :global(b) {
    font-weight: 900;
    color: #ffffff; /* Blanco brillante para que destaque del resto de números */
    text-shadow: 0 0 2px rgba(255, 255, 255, 0.4);
  }

  .cmd {
    color: #e4e4e7;
    font-size: 11px;
    white-space: normal;
    word-break: break-word;
    line-height: 1.25;
  }
  .linea-tirada.error .cmd {
    color: #ef4444;
    font-style: italic;
  }

  .total {
    color: #4ade80;
    font-size: 12px;
    float: right;
    text-indent: 0;
    margin-left: 6px;
    white-space: nowrap;
  }
</style>
