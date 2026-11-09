<!-- src/routes/componentes/panel-derecho/FichasActivas.svelte -->
<!-- Las instancias activas, en su orden. Como las filas del banco pero más grandes y con la misma
     miniatura de la ficha, en la forma de su tipo (cuadrado rojo, rombo azul, círculo verde). A la
     derecha, el «actual/máx» de los parámetros con forma «Resumen».
     - Pulsar = foco (con Ctrl, añadirla o quitarla del foco). Pulsar el hueco vacío = quitar el foco.
     - Arrastrar dentro de la lista = reordenar; al tablero = colocarla; al banco = eliminarla.
     - Doble clic = la foto de la ficha a resolución completa, en una ventanita junto al puntero (se cierra pulsando fuera).
     - Clic derecho = menú con «Eliminar instancia».
     Soltar aquí una ficha del banco la activa sin colocar; una del tablero, la quita de él. -->
<script lang="ts">
  import { partida, numeroDe, veParametro, type Instancia } from "$lib/partida.svelte";
  import { ajustesVista } from "$lib/ajustesVista.svelte";
  import { arrastreJugador } from "$lib/arrastreJugador.svelte";
  import { arrastre, moverEnLista } from "$lib/arrastreFicha.svelte";
  import { arrastreTirada } from "$lib/arrastreTirada.svelte";
  import { miniaturas } from "$lib/miniaturas.svelte";
  import { visorImagen } from "$lib/visorImagen.svelte";
  import MenuContextual from "../comunes/MenuContextual.svelte";

  // La miniatura de cada ficha activa (la misma que usan el banco, el arrastre y el tablero)
  $effect(() => {
    for (const i of partida.instanciasVisibles) miniaturas.asegurarFicha(i.base);
  });

  const FORMA_TIPO = { Enemigo: "cuadrado", NPC: "rombo", Jugador: "circulo" } as const;

  // Parámetros que se resumen en la tarjeta, con su color
  const resumidos = $derived(
    partida.definiciones.flatMap((d) =>
      d.tiene_maximo && d.representar_mapa && "Resumen" in d.representar_mapa
        ? [{ id: d.id, clave: d.clave, color: `rgb(${d.representar_mapa.Resumen.slice(0, 3).join(",")})` }]
        : []
    )
  );

  // Mientras se arrastra una tarjeta por la lista, la lista ya se ve con el orden nuevo
  const idArrastrada = $derived.by(() => {
    const a = arrastre.actual;
    return a && "instancia" in a.origen && a.origen.desde === "activas" && a.zona === "activas" ? a.origen.instancia.id_instancia : null;
  });
  // Con «Este mapa» solo las puestas en el tablero del mapa abierto (`data-indice` es la posición en ESTA lista)
  const visuales = $derived.by(() => {
    const lista = partida.instanciasVisibles;
    if (idArrastrada === null || arrastre.actual?.indice == null) return lista;
    const ids = moverEnLista(lista.map((i) => i.id_instancia), idArrastrada, arrastre.actual.indice);
    return ids.map((id) => lista.find((i) => i.id_instancia === id)!);
  });

  let menu = $state<{ x: number; y: number; instancia: Instancia } | null>(null);

  function dondeEsta(i: Instancia): string {
    if (typeof i.coordenadas !== "object" || i.id_mapa === null) return "Sin colocar";
    if (i.id_mapa === partida.mapaActual?.id) return "En este mapa";
    const mapa = partida.mapas.find((m) => m.id === i.id_mapa);
    return mapa ? `En el mapa ${mapa.nombre}` : "En otro mapa";
  }

  function alPulsar(e: MouseEvent, i: Instancia) {
    if (arrastre.esClicDeArrastre()) return;
    partida.seleccionarInstancia(i, e.ctrlKey || e.metaKey); // Ctrl: varias, como en el banco y el tablero
  }

  // Pulsar el hueco vacío de la lista (no una tarjeta) quita el foco
  function alPulsarFondo(e: PointerEvent) {
    if (e.button === 0 && !(e.target as Element).closest(".tarjeta")) partida.seleccionarInstancia(null);
  }
</script>

<div class="cabecera">
  <span class="titulo-mini">INSTANCIAS</span>
  <button
    type="button"
    class="filtro"
    class:activo={ajustesVista.soloMapaActual}
    aria-pressed={ajustesVista.soloMapaActual}
    data-conserva-foco
    title={ajustesVista.soloMapaActual
      ? "Solo las fichas puestas en el mapa abierto. Pulsa para ver todas"
      : "Pulsa para ver solo las fichas puestas en el mapa abierto"}
    onclick={() => ajustesVista.alternarSoloMapaActual()}
  >
    Este mapa
  </button>
</div>
<div
  class="scroll-fichas"
  data-destino="activas"
  data-conserva-foco
  class:destino={arrastre.actual?.zona === "activas" && idArrastrada === null}
  role="list"
  onpointerdown={alPulsarFondo}
>
  {#each visuales as ficha, i (ficha.id_instancia)}
    {@const forma = FORMA_TIPO[ficha.base.tipo] ?? "cuadrado"}
    <button
      type="button"
      class="tarjeta"
      class:seleccionado={partida.estaSeleccionada(ficha.id_instancia)}
      class:caida={partida.estaCaida(ficha)}
      class:foco-ajeno={partida.idsFocoAjeno.includes(ficha.id_instancia) && !partida.estaSeleccionada(ficha.id_instancia)}
      class:arrastrada={idArrastrada === ficha.id_instancia}
      class:recibe-jugador={arrastreJugador.actual?.idEntidad === ficha.base.id}
      data-indice={i}
      data-id-entidad={ficha.base.id}
      onpointerdown={(e) => arrastre.preparar(e, { instancia: ficha, desde: "activas" })}
      onclick={(e) => alPulsar(e, ficha)}
      ondblclick={(e) => visorImagen.abrir(ficha.base.foto_url, e.clientX, e.clientY)}
      oncontextmenu={(e) => {
        e.preventDefault();
        if (partida.controla(ficha)) menu = { x: e.clientX, y: e.clientY, instancia: ficha };
      }}
      title="Clic: foco (Ctrl: varias) · Doble clic: ver la foto · Arrastrar: reordenar o llevar al tablero · Clic derecho: eliminar"
    >
      <div class="foto {forma}">
        {#if ficha.base.foto_url}
          {@const mini = miniaturas.deFicha(ficha.base)}
          {#if mini}
            <img src={mini} alt="" draggable="false" />
          {/if}
        {:else}
          <span class="inicial">{ficha.base.nombre.charAt(0).toUpperCase()}</span>
        {/if}
      </div>

      <div class="texto">
        <span class="nombre">{ficha.base.nombre}</span>
        <span class="donde">{dondeEsta(ficha)}</span>
      </div>

      {#if resumidos.length}
        <div class="resumen">
          {#each resumidos.filter((r) => veParametro(ficha, r.id)) as r (r.id)}
            <!-- Una tirada soltada aquí cambia este valor (ver $lib/arrastreTirada) -->
            <span
              style:color={r.color}
              title={r.clave}
              data-valor="{ficha.id_instancia}:{r.id}"
              class:destino-tirada={arrastreTirada.esDestino(ficha.id_instancia, r.id)}
            >{partida.valorMostrado(ficha, r.id)}/{numeroDe(ficha.base, r.id)}</span>
          {/each}
        </div>
      {/if}
    </button>
  {/each}

  {#if partida.instancias.length === 0}
    <span class="vacio">Arrastra aquí o al tablero una ficha del banco</span>
  {:else if visuales.length === 0}
    <span class="vacio">Ninguna ficha puesta en este mapa</span>
  {/if}
</div>

{#if menu}
  {@const elegida = menu.instancia}
  <div data-conserva-foco>
    <MenuContextual
      x={menu.x}
      y={menu.y}
      opciones={[{ texto: "Eliminar instancia", peligro: true, accion: () => void partida.borrarInstancia(elegida.id_instancia) }]}
      onCerrar={() => (menu = null)}
    />
  </div>
{/if}

<style>
  .cabecera { display: flex; justify-content: space-between; align-items: center; margin-bottom: 3px; }
  .titulo-mini { font-size: 10px; font-weight: bold; color: #71717a; display: block; }
  /* Mismo aspecto que los filtros rápidos del banco */
  .filtro {
    background: #1c1c1f;
    border: 1px solid #27272a;
    color: #a1a1aa;
    font-size: 10px;
    padding: 1px 6px;
    border-radius: 3px;
    cursor: pointer;
    line-height: normal;
  }
  .filtro:hover { background: #27272a; color: #f4f4f5; }
  .filtro.activo { background: #27272a; color: #ffffff; border-color: #3f3f46; font-weight: bold; }
  .scroll-fichas {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 4px;
    border: 1px dashed transparent;
    border-radius: 4px;
  }
  .scroll-fichas.destino { border-color: #60a5fa; background: rgba(96, 165, 250, 0.06); }

  .tarjeta {
    display: flex;
    align-items: center;
    gap: 10px;
    background: #1f1f23;
    padding: 6px 8px;
    border-radius: 5px;
    cursor: pointer;
    border: 1px solid transparent;
    width: 100%;
    color: #f4f4f5;
    font: inherit;
    text-align: left;
    flex-shrink: 0;
    touch-action: none;
  }
  .tarjeta:hover { background: #27272a; border-color: #3f3f46; }
  .tarjeta:focus-visible { outline: none; } /* el estado lo marca solo .seleccionado */
  .tarjeta.seleccionado { border-color: #3b82f6; }
  /* Sin vida (0 o negativa): en gris. Los textos de vida se siguen leyendo */
  .tarjeta.caida .foto, .tarjeta.caida .texto { opacity: 0.4; filter: grayscale(100%); }
  /* El foco de otro jugador (llega por red) */
  .tarjeta.foco-ajeno { border-color: #a855f7; border-style: dotted; }
  /* El hueco donde caerá la tarjeta que se está reordenando */
  .tarjeta.arrastrada { opacity: 0.35; border-style: dashed; border-color: #60a5fa; }
  /* Se está soltando encima un jugador (pasará a controlar esta ficha) */
  .tarjeta.recibe-jugador { border-color: #22c55e; background: rgba(34, 197, 94, 0.12); }

  /* Foto dentro de la forma de su tipo (como en el banco, más grande) */
  .foto {
    width: 38px;
    height: 38px;
    flex-shrink: 0;
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
    border: 2px solid;
    box-sizing: border-box;
  }
  .foto img { width: 100%; height: 100%; object-fit: cover; pointer-events: none; }
  .inicial { font-weight: bold; font-size: 15px; }
  .foto.cuadrado { border-color: #ef4444; background: #7f1d1d; border-radius: 4px; }
  .foto.circulo { border-color: #22c55e; background: #14532d; border-radius: 50%; }
  .foto.rombo {
    width: 30px;
    height: 30px;
    margin: 4px;
    transform: rotate(45deg);
    border-color: #3b82f6;
    background: #1e3a8a;
    border-radius: 3px;
  }
  .foto.rombo img, .foto.rombo .inicial { transform: rotate(-45deg) scale(1.42); }

  .texto { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .nombre { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; font-weight: bold; }
  .donde { font-size: 10px; color: #71717a; }

  .resumen { display: flex; flex-direction: column; align-items: flex-end; gap: 1px; font-size: 12px; font-weight: bold; }
  .resumen span { padding: 0 3px; border-radius: 3px; }
  .resumen .destino-tirada { background: rgba(250, 204, 21, 0.25); outline: 1px solid #facc15; }
  .vacio { color: #52525b; font-size: 10px; text-align: center; margin-top: 10px; }
</style>