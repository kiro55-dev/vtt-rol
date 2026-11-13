<!-- src/routes/componentes/VentanaColorFlecha.svelte -->
<!-- Ventana flotante (no modal) para elegir el color de MI flecha y de MI ping en el tablero: los demás me ven con él.
     Se elige igual que el color de la rejilla (el selector de color del navegador, con su rueda y sus valores RGB), con
     una muestra de la flecha y un botón para sortear otro. Cambia en vivo y se recuerda en este equipo (ver
     ajustesVista). Lo tienen el host y cada jugador. -->
<script lang="ts">
  import { ajustesVista, ayuda } from "$lib/ajustesVista.svelte";
  import { aHex, deHex, rgbCss } from "$lib/colorMarca";
  import { moverVentana } from "$lib/moverVentana";

  let { onCerrar }: { onCerrar: () => void } = $props();

  let ventana = $state<HTMLDivElement | null>(null);
  let pos = $state({ x: 260, y: 120 });

  const color = $derived(ajustesVista.colorFlecha);

  function elegir(e: Event) {
    const c = deHex((e.currentTarget as HTMLInputElement).value);
    if (c) ajustesVista.cambiarColorFlecha(c);
  }

  function alTeclear(e: KeyboardEvent) {
    if (e.key === "Escape") onCerrar();
  }
</script>

<div
  class="ventana-flotante"
  style="left: {pos.x}px; top: {pos.y}px;"
  role="dialog"
  aria-label="Color de mi flecha"
  tabindex="-1"
  onkeydown={alTeclear}
  bind:this={ventana}
>
  <div class="cabecera" role="presentation" onpointerdown={(e) => moverVentana(e, pos, ventana)}>
    <span class="icono-drag">⋮⋮</span>
    <span class="titulo">Color de mi flecha</span>
    <button type="button" class="btn-cerrar" aria-label="Cerrar" onclick={onCerrar}>✕</button>
  </div>

  <div class="cuerpo">
    <!-- La muestra: la flecha tal cual se verá en el tablero (contorno oscuro y punta) -->
    <svg class="muestra" viewBox="0 0 220 56" role="img" aria-label="Muestra de la flecha">
      <rect x="8" y="14" width="28" height="28" rx="2" fill={rgbCss(color, 0.4)} />
      <path d="M22 28 H172" stroke="rgba(0,0,0,0.6)" stroke-width="10" stroke-linecap="round" fill="none" />
      <path d="M160 14 L196 28 L160 42 Z" stroke="rgba(0,0,0,0.6)" stroke-width="4" stroke-linejoin="round" fill="rgba(0,0,0,0.6)" />
      <path d="M22 28 H172" stroke={rgbCss(color)} stroke-width="6" stroke-linecap="round" fill="none" />
      <path d="M162 16 L192 28 L162 40 Z" fill={rgbCss(color)} />
      <text x="204" y="32" fill="#fff" stroke="rgba(0,0,0,0.8)" stroke-width="3" paint-order="stroke" font-size="13" font-weight="bold">7</text>
    </svg>

    <!-- Igual que «Color:» de la barra de la rejilla -->
    <div class="fila">
      <div class="control">
        <label for="color-flecha">Color:</label>
        <input id="color-flecha" type="color" value={aHex(color)} oninput={elegir} />
      </div>
      <button type="button" class="btn-aleatorio" title={ayuda("Sortea otro color")} onclick={() => ajustesVista.colorFlechaAleatorio()}>Aleatorio</button>
    </div>

    <p class="ayuda">Los demás ven tus flechas y tus pings de este color. Se guarda en este equipo.</p>
  </div>
</div>

<style>
  .ventana-flotante {
    position: fixed; z-index: 1000; width: 260px;
    display: flex; flex-direction: column; overflow: hidden;
    background: #18181b; border: 1px solid #3f3f46; border-radius: 8px;
    box-shadow: 0 14px 35px rgba(0, 0, 0, 0.75);
    user-select: none; font-size: 13px; color: #f4f4f5;
  }

  .cabecera {
    display: flex; align-items: center; gap: 6px; padding: 6px 10px; cursor: grab;
    background: #27272a; border-bottom: 1px solid #3f3f46; font-size: 11px;
  }
  .cabecera:active { cursor: grabbing; }
  .icono-drag { color: #71717a; font-size: 12px; }
  .titulo { font-weight: bold; flex: 1; }
  .btn-cerrar { background: none; border: none; color: #a1a1aa; cursor: pointer; font-size: 13px; }
  .btn-cerrar:hover { color: white; }

  .cuerpo { padding: 10px; display: flex; flex-direction: column; gap: 10px; }
  .muestra { width: 100%; height: 56px; background: #09090b; border: 1px solid #27272a; border-radius: 5px; }

  .fila { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .control { display: flex; align-items: center; gap: 6px; }
  .btn-aleatorio {
    background: #27272a; border: 1px solid #3f3f46; border-radius: 4px; color: #f4f4f5; cursor: pointer; padding: 4px 10px; font-size: 11px;
  }
  .btn-aleatorio:hover { background: #3f3f46; }
  .ayuda { margin: 0; color: #71717a; font-size: 10px; }
</style>
