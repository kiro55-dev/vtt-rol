<!-- cliente-web/src/ListaFichas.svelte -->
<!-- «Tus fichas» (las que el host te ha dado) y «En el mapa» (las demás que ves), con el actual/máx de los
     parámetros con resumen que puedes ver. -->
<script lang="ts">
  import type { Instancia } from "$lib/partida.svelte";
  import type { Conexion } from "./conexion.svelte";
  import { maximo, valorActual } from "./vista";

  let { conexion }: { conexion: Conexion } = $props();

  const COLOR = { Enemigo: "#ef4444", NPC: "#3b82f6", Jugador: "#22c55e" } as const;
  const vista = $derived(conexion.vista);
  const mias = $derived(new Set(vista.entidades.map((e) => e.id)));
  const resumidos = $derived(
    vista.definiciones.filter((d) => d.tiene_maximo && d.representar_mapa && "Resumen" in d.representar_mapa)
  );

  const enMapa = (i: Instancia) => vista.mapa !== null && i.id_mapa === vista.mapa.id && typeof i.coordenadas === "object";
  const instanciasMias = $derived(vista.instancias.filter((i) => mias.has(i.base.id)));
  const otras = $derived(vista.instancias.filter((i) => !mias.has(i.base.id) && enMapa(i)));
</script>

{#snippet tarjeta(i: Instancia)}
  <div class="tarjeta">
    <span class="tipo" style:background={COLOR[i.base.tipo]}></span>
    <span class="nombre">{i.base.nombre}</span>
    {#if !enMapa(i)}<span class="donde">fuera del mapa</span>{/if}
    {#each resumidos.filter((d) => i.base.parametros.some((p) => p.id_definicion === d.id)) as d (d.id)}
      <span class="valor" title={d.clave}>{valorActual(i, d.id)}/{maximo(i, d.id)}</span>
    {/each}
  </div>
{/snippet}

<div class="lista">
  <span class="titulo">TUS FICHAS</span>
  {#each instanciasMias as i (i.id_instancia)}
    {@render tarjeta(i)}
  {:else}
    <span class="vacio">
      {vista.entidades.length > 0 ? "Tus fichas no están en juego ahora" : "El host aún no te ha dado ninguna ficha"}
    </span>
  {/each}

  <span class="titulo">EN EL MAPA</span>
  {#each otras as i (i.id_instancia)}
    {@render tarjeta(i)}
  {:else}
    <span class="vacio">{vista.mapa ? "No hay nadie más" : "El host no está enseñando ningún mapa"}</span>
  {/each}
</div>

<style>
  .lista { flex: 1; overflow-y: auto; padding: 8px; display: flex; flex-direction: column; gap: 4px; }
  .titulo { font-size: 10px; font-weight: bold; color: #71717a; margin-top: 6px; }
  .tarjeta { display: flex; align-items: center; gap: 6px; background: #1f1f23; border-radius: 4px; padding: 5px 6px; }
  .tipo { width: 8px; height: 8px; border-radius: 2px; flex-shrink: 0; }
  .nombre { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: bold; }
  .donde { color: #71717a; font-size: 10px; }
  .valor { color: #22c55e; font-weight: bold; }
  .vacio { color: #52525b; font-size: 11px; }
</style>
