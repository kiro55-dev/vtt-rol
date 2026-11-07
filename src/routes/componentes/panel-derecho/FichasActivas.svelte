<!-- src/routes/componentes/panel-derecho/FichasActivas.svelte -->
<script lang="ts">
  import { partida, ID_PARAMETRO, numeroDe, valorActual } from "$lib/partida.svelte";

  // Por ahora solo se muestra la vida de serie (parámetro fijo)
  const vida = (f: (typeof partida.instancias)[number]) => valorActual(f, ID_PARAMETRO.vida);

  // Cada grupo se pinta igual; solo cambian el título, el color, el icono y si se muestra la vida máxima.
  const GRUPOS = [
    { tipo: "Enemigo", titulo: "ENEMIGOS", color: "rojo", icono: "cuadrado", mostrarMax: true },
    { tipo: "NPC", titulo: "NPCs", color: "azul", icono: "rombo", mostrarMax: false },
    { tipo: "Jugador", titulo: "JUGADORES", color: "verde", icono: "circulo", mostrarMax: true }
  ] as const;

  let grupos = $derived(
    GRUPOS.map((g) => ({ ...g, fichas: partida.instancias.filter((i) => i.base.tipo === g.tipo) }))
  );
</script>

<span class="titulo-mini">FICHAS ACTIVAS</span>
<div class="scroll-fichas">
  {#each grupos as g}
    <div class="grupo {g.color}">{g.titulo}</div>
    {#each g.fichas as ficha}
      <button
        type="button"
        class="fila-ficha"
        class:muerto={vida(ficha) <= 0}
        class:seleccionado={partida.idInstanciaSeleccionada === ficha.id_instancia}
        onclick={() => partida.seleccionarInstancia(ficha)}
      >
        <span class="icono {g.icono}"></span>
        <span class="nombre">{ficha.base.nombre}</span>
        <span class="pv">{g.mostrarMax ? `${vida(ficha)}/${numeroDe(ficha.base, ID_PARAMETRO.vida)}` : vida(ficha)}</span>
      </button>
    {/each}
  {/each}
</div>

<style>
  .titulo-mini { font-size: 10px; font-weight: bold; color: #71717a; margin-bottom: 3px; display: block; }
  .scroll-fichas { flex: 1; overflow-y: auto; display: flex; flex-direction: column; gap: 3px; }
  .grupo { font-size: 10px; font-weight: bold; margin-top: 4px; border-bottom: 1px solid; }
  .grupo.rojo { color: #ef4444; border-color: #ef4444; }
  .grupo.azul { color: #3b82f6; border-color: #3b82f6; }
  .grupo.verde { color: #22c55e; border-color: #22c55e; }
  .fila-ficha {
    display: flex; align-items: center; gap: 6px; background: #1f1f23; padding: 4px 6px; border-radius: 4px; cursor: pointer;
    /* reset de botón para que se vea igual que el div anterior */
    border: 1px solid transparent; width: 100%; color: inherit; font: inherit; text-align: left;
  }
  .fila-ficha.seleccionado { border-color: #3b82f6; }
  .fila-ficha.muerto { opacity: 0.35; filter: grayscale(100%); }
  .icono { width: 10px; height: 10px; flex-shrink: 0; }
  .cuadrado { background: #ef4444; }
  .rombo { background: #3b82f6; transform: rotate(45deg); }
  .circulo { background: #22c55e; border-radius: 50%; }
  .nombre { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 11px; }
</style>
