<!-- src/routes/componentes/panel-derecho/ParametrosFicha.svelte -->
<script lang="ts">
  import { partida, ID_PARAMETRO, numeroDe, valorActual, type Cuerpo } from "$lib/partida.svelte";
  // Por ahora muestra los parámetros de serie (ids fijos); no los definidos por el usuario.

  const textoCuerpo = (c: Cuerpo) => (typeof c === "object" ? `Grande (${c.Grande.join("x")})` : "1x1");

  let ficha = $derived(partida.instanciaSeleccionada);

  let parametros = $derived(
    ficha
      ? [
          { etiqueta: "V (Velocidad)", valor: `${numeroDe(ficha.base, ID_PARAMETRO.velocidad)} m` },
          { etiqueta: "RA (Rango Arma)", valor: `${numeroDe(ficha.base, ID_PARAMETRO.rangoArma)} m` },
          { etiqueta: "Cuerpo", valor: textoCuerpo(ficha.base.cuerpo) },
          { etiqueta: "Armadura", valor: String(numeroDe(ficha.base, ID_PARAMETRO.armadura)) }
        ]
      : []
  );
</script>

<div class="caja">
  <span class="titulo-mini">PARÁMETROS</span>
  {#if ficha}
    <div class="caja-parametros">
      {#each parametros as p}
        <div class="param">
          <span class="label">{p.etiqueta}</span>
          <span class="valor-destacado">{p.valor}</span>
        </div>
      {/each}

      <div class="param vida">
        <span class="label">Vida:</span>
        <div class="control-vida">
          <button onclick={() => partida.modificarValorActual(ficha.id_instancia, ID_PARAMETRO.vida, -1)}>-</button>
          <span class="valor-destacado">{valorActual(ficha, ID_PARAMETRO.vida)} / {numeroDe(ficha.base, ID_PARAMETRO.vida)}</span>
          <button onclick={() => partida.modificarValorActual(ficha.id_instancia, ID_PARAMETRO.vida, 1)}>+</button>
        </div>
      </div>
    </div>
  {:else}
    <div class="sin-seleccion">Pulsa una ficha para ver sus parámetros</div>
  {/if}
</div>

<style>
  .caja {
    flex: 1;
    min-height: 0;
    background: #1b1b1f;
    border: 1px solid #27272a;
    border-radius: 6px;
    padding: 6px;
    display: flex;
    flex-direction: column;
  }
  .titulo-mini { font-size: 10px; font-weight: bold; color: #71717a; margin-bottom: 3px; display: block; }
  .caja-parametros { display: grid; grid-template-columns: 1fr 1fr; gap: 4px; }
  .param { background: #27272a; padding: 5px; border-radius: 4px; }
  .param.vida { grid-column: span 2; display: flex; justify-content: space-between; align-items: center; }
  .label { color: #a1a1aa; font-size: 10px; display: block; }
  .valor-destacado { font-size: 13px; font-weight: bold; color: #f4f4f5; }
  .control-vida { display: flex; align-items: center; gap: 6px; }
  .control-vida button {
    background: #3f3f46; color: white; border: none; width: 22px; height: 22px; border-radius: 3px; cursor: pointer; font-weight: bold;
  }
  .sin-seleccion { color: #71717a; text-align: center; margin: auto 0; font-size: 11px; }
</style>
