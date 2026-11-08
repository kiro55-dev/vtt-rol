<!-- src/routes/componentes/comunes/FantasmaFicha.svelte -->
<!-- La ficha que se está arrastrando sigue al cursor. Sobre el tablero no se pinta: allí se ve el
     rectángulo amarillo de las casillas donde caerá. -->
<script lang="ts">
  import { arrastre, baseDe } from "$lib/arrastreFicha.svelte";
  import { arrastreTirada } from "$lib/arrastreTirada.svelte";
  import { miniaturas } from "$lib/miniaturas.svelte";
  import { COLOR_TIPO } from "$lib/fichasTablero";

  const a = $derived(arrastre.actual);
  const base = $derived(a ? baseDe(a.origen) : null);
  // Con varias fichas del banco (selección con Ctrl) se ve la primera con «×N»
  const cuantas = $derived(a && "entidades" in a.origen ? a.origen.entidades.length : 1);
  // La miniatura de la ficha (la misma que usa todo lo demás)
  $effect(() => {
    if (base?.foto_url) miniaturas.asegurarFicha(base);
  });
  const mini = $derived(base?.foto_url ? miniaturas.deFicha(base) : "");
  // Qué pasará al soltar aquí, para que no sea una sorpresa
  const pista = $derived.by(() => {
    if (!a) return "";
    const esFicha = !("instancia" in a.origen);
    if (a.zona === "activas") {
      if (esFicha) return cuantas > 1 ? `Activar las ${cuantas} sin colocar` : "Activar sin colocar";
      return "desde" in a.origen && a.origen.desde === "activas" ? "" : "Quitar del tablero";
    }
    if (a.zona === "banco") return esFicha ? "" : "Eliminar instancia";
    return "";
  });
</script>

{#if arrastreTirada.actual}
  {@const t = arrastreTirada.actual}
  <div class="tirada" class:resta={t.delta <= 0} style:left="{t.x}px" style:top="{t.y}px">
    {t.delta > 0 ? "+" : "−"}{Math.abs(t.delta)}
  </div>
{/if}

{#if a && base && a.zona !== "tablero"}
  <div class="fantasma" style:left="{a.x}px" style:top="{a.y}px" style:border-color={COLOR_TIPO[base.tipo]}>
    {#if base.foto_url}
      {#if mini}
        <img src={mini} alt="" draggable="false" />
      {/if}
    {:else}
      <span>{base.nombre.charAt(0).toUpperCase()}</span>
    {/if}
    {#if cuantas > 1}
      <span class="cuantas">×{cuantas}</span>
    {/if}
  </div>
  {#if pista}
    <div class="pista" style:left="{a.x}px" style:top="{a.y + 30}px">{pista}</div>
  {/if}
{/if}

<style>
  /* El número de la tirada que se arrastra: rojo si quita, verde si suma */
  .tirada {
    position: fixed;
    z-index: 1000;
    transform: translate(12px, -50%);
    font-size: 16px;
    font-weight: bold;
    padding: 2px 8px;
    border-radius: 4px;
    background: rgba(0, 0, 0, 0.85);
    color: #4ade80;
    border: 1px solid #4ade80;
    pointer-events: none;
  }
  .tirada.resta { color: #f87171; border-color: #f87171; }
  .fantasma {
    position: fixed;
    z-index: 1000;
    width: 44px;
    height: 44px;
    transform: translate(-50%, -50%);
    border: 3px solid;
    border-radius: 4px;
    background: #18181b;
    overflow: hidden;
    opacity: 0.85;
    pointer-events: none;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #f4f4f5;
    font-weight: bold;
  }
  .cuantas {
    position: absolute;
    right: 0;
    bottom: 0;
    background: #3b82f6;
    color: white;
    font-size: 10px;
    line-height: 1;
    padding: 2px 4px;
    border-radius: 6px;
  }
  .fantasma img { width: 100%; height: 100%; object-fit: cover; }
  .pista {
    position: fixed;
    z-index: 1000;
    transform: translateX(-50%);
    background: rgba(0, 0, 0, 0.8);
    color: #f4f4f5;
    font-size: 11px;
    padding: 2px 6px;
    border-radius: 3px;
    white-space: nowrap;
    pointer-events: none;
  }
</style>
