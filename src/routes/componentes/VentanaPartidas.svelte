<!-- src/routes/componentes/VentanaPartidas.svelte -->
<!-- Ventana flotante (no modal) para alternar entre partidas o crear una nueva. Cada partida es una
     carpeta; al cambiar, Rust guarda antes la que se deja. La lógica vive en $lib/gestorPartidas. -->
<script lang="ts">
  import { onMount } from "svelte";
  import { partida } from "$lib/partida.svelte";
  import { GestorPartidas } from "$lib/gestorPartidas.svelte";
  import { moverVentana } from "$lib/moverVentana";

  let { onCerrar }: { onCerrar: () => void } = $props();

  const gestor = new GestorPartidas();
  let ventana = $state<HTMLDivElement | null>(null);
  let pos = $state({ x: 300, y: 100 });
  // La partida cuyo borrado se está confirmando (nada se borra sin pasar por aquí)
  let confirmando = $state<string | null>(null);

  async function borrar(nombre: string) {
    if (await gestor.borrar(nombre)) confirmando = null;
  }

  onMount(() => {
    ventana?.focus(); // para que Escape funcione sin tocar la ventana
    gestor.cargar();
  });

  // El campo de renombrar nace con el foco y el nombre seleccionado
  function enfocar(el: HTMLInputElement) {
    el.focus();
    el.select();
  }

  function alTeclearNombre(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      void gestor.guardarRenombrado();
    } else if (e.key === "Escape") {
      e.stopPropagation(); // cancela el renombrado, no cierra la ventana
      gestor.cancelarRenombrar();
    }
  }

  function alTeclear(e: KeyboardEvent) {
    if (e.key === "Escape") {
      if (confirmando) confirmando = null; // primero se cancela el borrado
      else onCerrar();
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  class="ventana-flotante"
  style="left: {pos.x}px; top: {pos.y}px;"
  role="dialog"
  aria-label="Partidas"
  tabindex="-1"
  onkeydown={alTeclear}
  bind:this={ventana}
>
  <div class="cabecera" role="presentation" onpointerdown={(e) => moverVentana(e, pos, ventana)}>
    <span class="icono-drag">⋮⋮</span>
    <span class="titulo">Partidas</span>
    <button type="button" class="btn-cerrar" aria-label="Cerrar" onclick={onCerrar}>✕</button>
  </div>

  <div class="cuerpo">
    <p class="ayuda">Cada partida es una carpeta. Al cambiar de una a otra se guarda antes la que dejas.</p>

    <div class="lista">
      {#each partida.partidas.lista as nombre (nombre)}
        {@const abierta = nombre === partida.partidas.actual}
        {#if gestor.renombrando === nombre}
          <!-- Renombrando: Enter guarda, Escape cancela -->
          <div class="item">
            <input
              type="text"
              class="input-texto"
              maxlength="40"
              aria-label="Nuevo nombre de la partida {nombre}"
              disabled={gestor.ocupado}
              bind:value={gestor.textoNuevo}
              onkeydown={alTeclearNombre}
              use:enfocar
            />
            <button type="button" class="btn-guardar" disabled={gestor.ocupado} onclick={() => gestor.guardarRenombrado()}>Guardar</button>
            <button type="button" class="btn-cancelar" disabled={gestor.ocupado} onclick={() => gestor.cancelarRenombrar()}>Cancelar</button>
          </div>
        {:else}
        <div class="item">
          <button
            type="button"
            class="fila"
            class:abierta
            disabled={gestor.ocupado}
            aria-current={abierta}
            onclick={() => gestor.abrir(nombre)}
          >
            <span class="nombre">{nombre}</span>
            {#if abierta}<span class="marca">abierta</span>{/if}
          </button>
          <button
            type="button"
            class="btn-borrar"
            title="Cambiar el nombre de esta partida"
            aria-label="Renombrar la partida {nombre}"
            disabled={gestor.ocupado}
            onclick={() => {
              confirmando = null;
              gestor.empezarRenombrar(nombre);
            }}>Renombrar</button
          >
          <!-- «Borrar» (no una papelera: no hay vuelta atrás). Vale también la abierta: se pasa a otra antes -->
          <button
            type="button"
            class="btn-borrar"
            title="Borrar esta partida…"
            aria-label="Borrar la partida {nombre}"
            disabled={gestor.ocupado}
            onclick={() => {
              gestor.cancelarRenombrar();
              confirmando = nombre;
            }}>Borrar</button
          >
        </div>
        {/if}
      {:else}
        <div class="vacio">Cargando…</div>
      {/each}
    </div>

    {#if confirmando}
      <!-- Confirmación en rojo: borrar una partida no se puede deshacer -->
      <div class="peligro" role="alertdialog" aria-label="Confirmar el borrado de la partida">
        <p class="aviso-rojo">¿Borrar la partida «{confirmando}»?</p>
        <p class="aviso-rojo">
          Se perderán <b>para siempre</b> todos sus mapas, fichas, imágenes, archivos, jugadores e historial. No se puede
          deshacer.
        </p>
        {#if confirmando === partida.partidas.actual}
          <p class="aviso-rojo">Es la partida abierta: al borrarla se abrirá otra (o una nueva vacía si no hay más).</p>
        {/if}
        <div class="acciones">
          <button type="button" class="btn-permanente" disabled={gestor.ocupado} onclick={() => borrar(confirmando!)}>
            {gestor.ocupado ? "BORRANDO…" : "BORRAR PERMANENTEMENTE"}
          </button>
          <!-- Mientras se borra no hay nada que cancelar: el borrado es inmediato -->
          {#if !gestor.ocupado}
            <button type="button" class="btn-cancelar" onclick={() => (confirmando = null)}>Cancelar</button>
          {/if}
        </div>
      </div>
    {/if}

    <div class="nueva">
      <input
        type="text"
        class="input-texto"
        maxlength="40"
        placeholder="Nombre de la nueva partida"
        aria-label="Nombre de la nueva partida"
        disabled={gestor.ocupado}
        bind:value={gestor.nombreNuevo}
        onkeydown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            gestor.crear();
          }
        }}
      />
      <button type="button" class="btn-crear" onclick={() => gestor.crear()} disabled={gestor.ocupado}>
        + Crear
      </button>
    </div>

    {#if gestor.ocupado}
      <p class="estado" role="status">Guardando y cambiando de partida…</p>
    {/if}
    {#if gestor.error}
      <p class="error" role="alert">{gestor.error}</p>
    {/if}
  </div>
</div>

<style>
  .ventana-flotante {
    position: fixed; z-index: 1000; width: 340px;
    display: flex; flex-direction: column; overflow: hidden;
    background: #18181b; border: 1px solid #3f3f46; border-radius: 8px;
    box-shadow: 0 14px 35px rgba(0, 0, 0, 0.75);
    user-select: none; font-size: 11px; color: #f4f4f5;
  }

  .cabecera {
    display: flex; align-items: center; gap: 6px; padding: 6px 10px; cursor: grab;
    background: #27272a; border-bottom: 1px solid #3f3f46;
  }
  .cabecera:active { cursor: grabbing; }
  .icono-drag { color: #71717a; font-size: 12px; }
  .titulo { font-weight: bold; flex: 1; }
  .btn-cerrar { background: none; border: none; color: #a1a1aa; cursor: pointer; font-size: 13px; }
  .btn-cerrar:hover { color: white; }

  .cuerpo { padding: 10px; display: flex; flex-direction: column; gap: 8px; }
  .ayuda { margin: 0; color: #a1a1aa; font-size: 10px; }

  .lista { display: flex; flex-direction: column; gap: 3px; max-height: 220px; overflow-y: auto; }
  .fila {
    display: flex; align-items: center; gap: 8px; padding: 6px 8px; cursor: pointer; text-align: left;
    background: #111113; border: 1px solid #27272a; border-radius: 5px; color: inherit; font-size: 11px;
  }
  .fila:hover:not(:disabled) { border-color: #3f3f46; background: #1c1c1f; }
  .fila.abierta { border-color: #3b82f6; cursor: default; }
  .fila:disabled { opacity: 0.6; cursor: wait; }
  .nombre { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .marca { color: #60a5fa; font-size: 9px; font-weight: 600; text-transform: uppercase; }
  .vacio { color: #71717a; font-size: 10px; padding: 8px; text-align: center; }

  .item { display: flex; gap: 4px; }
  .item .fila { flex: 1; min-width: 0; }
  .btn-borrar {
    background: #111113; border: 1px solid #27272a; border-radius: 5px; cursor: pointer; padding: 0 10px; font-size: 11px;
    color: #a1a1aa;
  }
  .btn-borrar:hover:not(:disabled) { border-color: #7f1d1d; background: #2a1215; color: #fca5a5; }
  .btn-borrar:disabled { opacity: 0.6; cursor: wait; }

  .peligro {
    display: flex; flex-direction: column; gap: 6px; padding: 10px; border: 1px solid #b91c1c; border-radius: 6px;
    background: rgba(220, 38, 38, 0.12);
  }
  .aviso-rojo { margin: 0; color: #f87171; font-size: 12px; }
  .acciones { display: flex; gap: 6px; align-items: center; }
  .btn-permanente {
    padding: 5px 10px; border-radius: 4px; border: none; cursor: pointer; font-size: 11px; font-weight: 700; letter-spacing: 0.3px;
    background: #dc2626; color: white;
  }
  .btn-permanente:hover:not(:disabled) { background: #b91c1c; }
  .btn-permanente:disabled { opacity: 0.6; cursor: wait; }
  .btn-guardar {
    padding: 4px 10px; border-radius: 4px; border: none; cursor: pointer; font-size: 11px; font-weight: 600; background: #2563eb; color: white;
  }
  .btn-guardar:hover:not(:disabled) { background: #1d4ed8; }
  .btn-guardar:disabled { opacity: 0.6; cursor: wait; }
  .btn-cancelar {
    padding: 5px 10px; border-radius: 4px; border: 1px solid #3f3f46; cursor: pointer; font-size: 11px; background: #27272a; color: #f4f4f5;
  }
  .btn-cancelar:hover:not(:disabled) { background: #3f3f46; }

  .nueva { display: flex; gap: 6px; }
  .input-texto {
    flex: 1; min-width: 0; padding: 4px 6px; border-radius: 4px;
    background: #09090b; border: 1px solid #3f3f46; color: white; font-size: 11px;
  }
  .btn-crear {
    padding: 4px 10px; border-radius: 4px; border: none; cursor: pointer; font-size: 11px; font-weight: 600;
    background: #2563eb; color: white;
  }
  .btn-crear:hover:not(:disabled) { background: #1d4ed8; }
  .btn-crear:disabled { opacity: 0.6; cursor: wait; }

  .estado { margin: 0; color: #a1a1aa; font-size: 10px; }
  .error { margin: 0; color: #f87171; font-size: 12px; }
</style>
