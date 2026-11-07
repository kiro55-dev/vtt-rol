<!-- src/routes/componentes/ventana-ficha/CampoNumero.svelte -->
<!-- Campo numérico que también se ajusta con la rueda del ratón al pasar por encima. -->
<script lang="ts">
  let {
    value = $bindable(),
    etiqueta = "",
    sufijo = "",
    min = 0,
    max = 32767,
    estrecho = false,
    titulo = "Rueda del ratón para subir/bajar"
  }: {
    value: number;
    etiqueta?: string;
    sufijo?: string;
    min?: number;
    max?: number;
    /** Más estrecho y centrado (para el cuerpo X × Y) */
    estrecho?: boolean;
    titulo?: string;
  } = $props();

  function alRueda(e: WheelEvent) {
    e.preventDefault();
    const paso = e.deltaY < 0 ? 1 : -1;
    value = Math.min(max, Math.max(min, Math.round(Number(value) || 0) + paso));
  }
</script>

<label title={titulo}>
  {etiqueta}
  <input type="number" class:estrecho {min} {max} bind:value onwheel={alRueda} />
  {#if sufijo}<span class="sufijo">{sufijo}</span>{/if}
</label>

<style>
  label { color: #a1a1aa; font-size: 10px; display: flex; align-items: center; gap: 4px; }
  input {
    width: 44px; background: #09090b; border: 1px solid #3f3f46; color: white;
    padding: 2px 4px; border-radius: 4px; font-size: 11px;
  }
  input.estrecho { width: 34px; text-align: center; }
  .sufijo { color: #71717a; font-size: 10px; }
</style>
