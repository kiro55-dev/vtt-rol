<!-- src/routes/componentes/BarraDados.svelte -->
<script>
  import { ayuda } from "$lib/ajustesVista.svelte";
  let { onSumarAlChat, onRestarDelChat, onLanzarTirada } = $props();

  let bonificador = $state(0);
  let esVentaja = $state(false);
  let esDesventaja = $state(false);

  let dados = $state([
    { tipo: "d4", cantidad: 1, color: "#22c55e" },
    { tipo: "d6", cantidad: 1, color: "#06b6d4" },
    { tipo: "d8", cantidad: 1, color: "#8b5cf6" },
    { tipo: "d10", cantidad: 1, color: "#ec4899" },
    { tipo: "d12", cantidad: 1, color: "#f97316" },
    { tipo: "d20", cantidad: 1, color: "#eab308" }
  ]);

  function rueda(e, d) {
    e.preventDefault();
    if (e.deltaY < 0) d.cantidad++;
    else if (d.cantidad > 1) d.cantidad--;
  }

  function toggleVentaja(e) {
    esVentaja = e.target.checked;
    if (esVentaja) esDesventaja = false; // Mutuamente excluyentes
  }

  function toggleDesventaja(e) {
    esDesventaja = e.target.checked;
    if (esDesventaja) esVentaja = false;
  }

  function formatearDado(d) {
    let sufijo = "";
    if (esVentaja) sufijo += "adv";
    if (esDesventaja) sufijo += "desv";
    return `${d.cantidad}${d.tipo}${sufijo}`;
  }
</script>

<div class="barra-dados">
  <div class="zona-dados">
    {#each dados as d}
      <button
        class="boton-dado"
        style="border-color: {d.color};"
        onwheel={(e) => rueda(e, d)}
        onclick={() => onSumarAlChat(formatearDado(d))}
        oncontextmenu={(e) => { e.preventDefault(); onRestarDelChat(formatearDado(d)); }}
        title={ayuda("Rueda: cantidad | Clic: sumar | Clic derecho: restar")}
      >
        <span class="cant">{d.cantidad}x</span>
        <span class="label">{d.tipo}</span>
      </button>
    {/each}
  </div>

  <!-- CHECKLISTS DE MODIFICADORES -->
  <div class="zona-modificadores">
    <label class="check-label" title={ayuda("Tira 2 veces cada dado y se queda con el mayor")}>
      <input type="checkbox" checked={esVentaja} onchange={toggleVentaja} />
      <span>Vent</span>
    </label>
    <label class="check-label" title={ayuda("Tira 2 veces cada dado y se queda con el menor")}>
      <input type="checkbox" checked={esDesventaja} onchange={toggleDesventaja} />
      <span>Desv</span>
    </label>
  </div>

  <div class="bonif">
    <span>+ Bonf:</span>
    <input type="number" bind:value={bonificador} />
  </div>

  <button class="btn-tirar" onclick={() => onLanzarTirada(bonificador)}>
    TIRAR
  </button>
</div>

<style>
  .barra-dados {
    display: flex;
    align-items: center;
    gap: 8px;
    background: #18181b;
    border: 1px solid #3f3f46;
    padding: 6px 12px;
    border-radius: 8px;
  }
  .zona-dados { display: flex; gap: 5px; }
  .boton-dado {
    background: #27272a;
    color: white;
    border: 2px solid;
    padding: 3px 6px;
    border-radius: 5px;
    cursor: pointer;
    display: flex;
    flex-direction: column;
    align-items: center;
    font-size: 11px;
    font-weight: bold;
    user-select: none;
  }
  .cant { font-size: 9px; color: #a1a1aa; }
  
  .zona-modificadores {
    display: flex;
    flex-direction: column;
    gap: 0px;
    border-left: 1px solid #3f3f46;
    border-right: 1px solid #3f3f46;
    padding: 0 8px;
  }
  .check-label {
    display: flex;
    align-items: center;
    gap: 2px;
    font-size: 10px;
    font-weight: bold;
    color: #e4e4e7;
    cursor: pointer;
    user-select: none;
  }
  .check-label input {
    cursor: pointer;
    accent-color: #eab308;
  }

  .bonif { display: flex; align-items: center; gap: 4px; font-size: 11px; }
  .bonif input { width: 40px; background: #09090b; border: 1px solid #3f3f46; color: white; padding: 4px; border-radius: 3px; }
  .btn-tirar {
    background: #00A2E8; color: white; border: none; font-weight: bold; padding: 6px 14px; border-radius: 4px; cursor: pointer;
  }
</style>