<!-- src/routes/componentes/panel-derecho/ParametrosFicha.svelte -->
<!-- Los parámetros de la instancia seleccionada, tal y como estén definidos en la partida (en su
     orden). Los que tienen máximo muestran «actual / máx» con − y + para cambiar el actual. -->
<script lang="ts">
  import { partida, numeroDe, veParametro, type Cuerpo } from "$lib/partida.svelte";
  import { arrastreTirada } from "$lib/arrastreTirada.svelte";
  import { esJugador } from "$lib/modo";

  const textoCuerpo = (c: Cuerpo) => (typeof c === "object" ? `${c.Grande[0] ?? 1}×${c.Grande[1] ?? 1}` : "1×1");

  const ficha = $derived(partida.instanciaSeleccionada);

  const parametros = $derived(
    ficha
      ? // Solo los que Rust manda: lo privado no se enseña
        partida.definiciones.filter((d) => veParametro(ficha, d.id)).map((d) => {
          const v = ficha.base.parametros.find((p) => p.id_definicion === d.id)?.valor;
          const unidad = d.unidad ? ` ${d.unidad}` : "";
          const texto = !v ? "—" : "Numero" in v ? `${v.Numero}${unidad}` : v.Texto || "—";
          return { id: d.id, clave: d.clave, tieneMaximo: d.tiene_maximo, texto, unidad };
        })
      : []
  );
</script>

<div class="caja" data-conserva-foco>
  <span class="titulo-mini">PARÁMETROS{ficha ? ` · ${ficha.base.nombre}` : ""}</span>
  {#if partida.idsSeleccionados.length > 1}
    <div class="sin-seleccion">{partida.seleccionadas.length} fichas con el foco</div>
  {:else if ficha}
    <div class="caja-parametros">
      {#each parametros as p (p.id)}
        {#if p.tieneMaximo}
          <!-- La rueda también lo cambia; se manda a Rust agrupado (ver partida.cambiarValorActual) -->
          <div
            class="param ancho"
            class:destino-tirada={arrastreTirada.esDestino(ficha.id_instancia, p.id)}
            data-valor="{ficha.id_instancia}:{p.id}"
            role="group"
            title={esJugador ? undefined : "Rueda o − / + para cambiarlo"}
            onwheel={(e) => {
              e.preventDefault();
              partida.cambiarValorActual(ficha.id_instancia, p.id, e.deltaY < 0 ? 1 : -1);
            }}
          >
            <span class="label">{p.clave}</span>
            <div class="control">
              {#if !esJugador}
                <button type="button" aria-label="Restar 1 a {p.clave}" onclick={() => partida.cambiarValorActual(ficha.id_instancia, p.id, -1)}>−</button>
              {/if}
              <span class="valor">{partida.valorMostrado(ficha, p.id)} / {numeroDe(ficha.base, p.id)}{p.unidad}</span>
              {#if !esJugador}
                <button type="button" aria-label="Sumar 1 a {p.clave}" onclick={() => partida.cambiarValorActual(ficha.id_instancia, p.id, 1)}>+</button>
              {/if}
            </div>
          </div>
        {:else}
          <div class="param">
            <span class="label">{p.clave}</span>
            <span class="valor">{p.texto}</span>
          </div>
        {/if}
      {/each}
      <div class="param">
        <span class="label">Cuerpo</span>
        <span class="valor">{textoCuerpo(ficha.base.cuerpo)}</span>
      </div>
    </div>
  {:else}
    <div class="sin-seleccion">Pulsa una ficha activa para ver sus parámetros</div>
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
  .titulo-mini {
    font-size: 10px; font-weight: bold; color: #71717a; margin-bottom: 3px; display: block;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .caja-parametros { display: grid; grid-template-columns: 1fr 1fr; gap: 4px; overflow-y: auto; min-height: 0; }
  .param { background: #27272a; padding: 5px; border-radius: 4px; min-width: 0; }
  .param.ancho { grid-column: span 2; display: flex; justify-content: space-between; align-items: center; }
  .param.destino-tirada { background: rgba(250, 204, 21, 0.25); outline: 1px solid #facc15; }
  .label { color: #a1a1aa; font-size: 10px; display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .valor { font-size: 13px; font-weight: bold; color: #f4f4f5; overflow-wrap: anywhere; }
  .control { display: flex; align-items: center; gap: 6px; }
  .control button {
    background: #3f3f46; color: white; border: none; width: 22px; height: 22px; border-radius: 3px; cursor: pointer; font-weight: bold;
  }
  .control button:hover { background: #52525b; }
  .sin-seleccion { color: #71717a; text-align: center; margin: auto 0; font-size: 11px; }
</style>
