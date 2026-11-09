<!-- src/routes/componentes/BarraRejilla.svelte -->
<script lang="ts">
  import { ayuda } from "$lib/ajustesVista.svelte";
  import { onMount } from "svelte";
  import { partida, LIMITES_REJILLA } from "$lib/partida.svelte";

  const { tamanoMin: TAMANO_MIN, tamanoMax: TAMANO_MAX, grosorMin: GROSOR_MIN, grosorMax: GROSOR_MAX } =
    LIMITES_REJILLA;
  const GROSOR_PASO = 1;

  let barraRef: HTMLDivElement | null = $state(null);

  // Quitar el foco y guardar de inmediato si se pulsa en cualquier otra parte
  onMount(() => {
    function onPointerDownGlobal(e: PointerEvent) {
      const target = e.target as Node;
      // Si el clic fue fuera de la barra de herramientas
      if (barraRef && !barraRef.contains(target)) {
        // Desenfocar explícitamente cualquier input activo
        if (document.activeElement instanceof HTMLElement) {
          document.activeElement.blur();
        }
        void partida.guardarRejilla();
      }
    }

    window.addEventListener("pointerdown", onPointerDownGlobal);
    return () => {
      window.removeEventListener("pointerdown", onPointerDownGlobal);
      void partida.guardarRejilla();
    };
  });

  function normalizarTamano() {
    const t = Math.round(Number(partida.rejilla.tamano));
    const fallback = partida.mapaActual?.rejilla || 50;
    partida.rejilla.tamano = Number.isFinite(t) && t > 0 ? Math.min(TAMANO_MAX, Math.max(TAMANO_MIN, t)) : fallback;
    partida.marcarRejillaModificada();
  }

  function normalizarGrosor() {
    const g = Number(partida.rejilla.grosor);
    const fallback = partida.mapaActual?.grosor ?? 1;
    partida.rejilla.grosor =
      Number.isFinite(g) && g >= GROSOR_MIN ? Math.min(GROSOR_MAX, Math.max(GROSOR_MIN, g)) : fallback;
    partida.marcarRejillaModificada();
  }

  function alRuedaGrosor(e: WheelEvent) {
    if (!partida.mapaActual) return;
    e.preventDefault();
    const delta = e.deltaY < 0 ? 1 : -1;

    // Si es 0 u otro número válido, lo respeta en lugar de caer al fallback
    const actual = Number.isFinite(Number(partida.rejilla.grosor))
      ? Math.round(Number(partida.rejilla.grosor))
      : (partida.mapaActual.grosor ?? 1);

    partida.rejilla.grosor = Math.min(GROSOR_MAX, Math.max(GROSOR_MIN, actual + delta));
    partida.marcarRejillaModificada();
  }

  // MODIFICACIÓN CON LA RUEDA DEL RATÓN
  function alRuedaTamano(e: WheelEvent) {
    if (!partida.mapaActual) return;
    e.preventDefault();
    const delta = e.deltaY < 0 ? 5 : -5;
    const actual = Math.round(Number(partida.rejilla.tamano) || partida.mapaActual.rejilla || 50);
    partida.rejilla.tamano = Math.min(TAMANO_MAX, Math.max(TAMANO_MIN, actual + delta));
    partida.marcarRejillaModificada();
  }
</script>

<div class="barra-herramientas" class:desactivada={!partida.mapaActual} bind:this={barraRef}>
  <button class="btn-lapiz" title={ayuda("Modo Dibujo")} disabled={!partida.mapaActual}>✏️</button>

  <div class="control" class:apagado={!partida.mapaActual}>
    <label for="rejilla-tamano">Rejilla:</label>
    <input
      id="rejilla-tamano"
      type="number"
      disabled={!partida.mapaActual}
      bind:value={partida.rejilla.tamano}
      oninput={() => partida.marcarRejillaModificada()}
      onchange={normalizarTamano}
      onwheel={alRuedaTamano}
      min={TAMANO_MIN}
      max={TAMANO_MAX}
      step="5"
      title={ayuda("Rueda del ratón para subir/bajar de 5 en 5")}
    />
    <span>px</span>
  </div>

  <div class="control" class:apagado={!partida.mapaActual}>
    <label for="rejilla-color">Color:</label>
    <input
      id="rejilla-color"
      type="color"
      disabled={!partida.mapaActual}
      bind:value={partida.rejilla.color}
      oninput={() => partida.marcarRejillaModificada()}
    />
  </div>

  <div class="control" class:apagado={!partida.mapaActual}>
    <label for="rejilla-grosor">Grosor:</label>
    <input
      id="rejilla-grosor"
      type="number"
      disabled={!partida.mapaActual}
      bind:value={partida.rejilla.grosor}
      oninput={() => partida.marcarRejillaModificada()}
      onchange={normalizarGrosor}
      onwheel={alRuedaGrosor}
      min={GROSOR_MIN}
      max={GROSOR_MAX}
      step={GROSOR_PASO}
      title={ayuda("Rueda del ratón para subir/bajar de 1 en 1")}
    />
  </div>

  <div class="control" class:apagado={!partida.mapaActual}>
    <label for="rejilla-opacidad">Opacidad:</label>
    <input
      id="rejilla-opacidad"
      type="range"
      disabled={!partida.mapaActual}
      min="0"
      max="1"
      step="0.05"
      bind:value={partida.rejilla.opacidad}
      oninput={() => partida.marcarRejillaModificada()}
    />
    <span class="porcentaje">{Math.round(partida.rejilla.opacidad * 100)}%</span>
  </div>
</div>

<style>
  .barra-herramientas {
    display: flex;
    align-items: center;
    gap: 15px;
    background-color: #262626;
    padding: 6px 16px;
    border-radius: 6px;
    font-size: 13px;
    border: 1px solid #3f3f46;
    user-select: none;
  }
  .barra-herramientas.desactivada {
    opacity: 0.5;
    pointer-events: none;
  }
  .control { display: flex; align-items: center; gap: 6px; }
  .control.apagado { opacity: 0.4; }
  .control input[type="number"] {
    width: 50px;
    background: #18181b;
    color: white;
    border: 1px solid #52525b;
    border-radius: 4px;
    padding: 2px 4px;
  }
  .porcentaje { min-width: 32px; text-align: right; }
  .btn-lapiz { background: none; border: none; cursor: pointer; font-size: 16px; }
</style>