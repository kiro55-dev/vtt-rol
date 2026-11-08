<!-- src/routes/componentes/panel-derecho/FichasActivas.svelte -->
<!-- Las instancias activas, en su orden. Como las filas del banco pero más grandes y con la foto a
     resolución completa, en la forma de su tipo (cuadrado rojo, rombo azul, círculo verde). A la
     derecha, el «actual/máx» de los parámetros con forma «Resumen».
     - Pulsar = foco (con Shift, añadirla o quitarla del foco). Pulsar el hueco vacío = quitar el foco.
     - Arrastrar dentro de la lista = reordenar; al tablero = colocarla; al banco = eliminarla.
     - Clic derecho = menú con «Eliminar instancia».
     Soltar aquí una ficha del banco la activa sin colocar; una del tablero, la quita de él. -->
<script lang="ts">
  import { partida, numeroDe, type Instancia } from "$lib/partida.svelte";
  import { arrastre, moverEnLista } from "$lib/arrastreFicha.svelte";
  import { urlDeImagen } from "$lib/imagenes";
  import MenuContextual from "../comunes/MenuContextual.svelte";

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
  const visuales = $derived.by(() => {
    const lista = partida.instanciasOrdenadas;
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
    partida.seleccionarInstancia(i, e.shiftKey);
  }

  // Pulsar el hueco vacío de la lista (no una tarjeta) quita el foco
  function alPulsarFondo(e: PointerEvent) {
    if (e.button === 0 && !(e.target as Element).closest(".tarjeta")) partida.seleccionarInstancia(null);
  }
</script>

<span class="titulo-mini">FICHAS ACTIVAS</span>
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
      class:arrastrada={idArrastrada === ficha.id_instancia}
      data-indice={i}
      onpointerdown={(e) => arrastre.preparar(e, { instancia: ficha, desde: "activas" })}
      onclick={(e) => alPulsar(e, ficha)}
      oncontextmenu={(e) => {
        e.preventDefault();
        menu = { x: e.clientX, y: e.clientY, instancia: ficha };
      }}
      title="Clic: foco (Shift: varias) · Arrastrar: reordenar o llevar al tablero · Clic derecho: eliminar"
    >
      <div class="foto {forma}">
        {#if ficha.base.foto_url}
          <img src={urlDeImagen(ficha.base.foto_url)} alt="" draggable="false" />
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
          {#each resumidos as r (r.id)}
            <span style:color={r.color} title={r.clave}>{partida.valorMostrado(ficha, r.id)}/{numeroDe(ficha.base, r.id)}</span>
          {/each}
        </div>
      {/if}
    </button>
  {/each}

  {#if partida.instancias.length === 0}
    <span class="vacio">Arrastra aquí o al tablero una ficha del banco</span>
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
  .titulo-mini { font-size: 10px; font-weight: bold; color: #71717a; margin-bottom: 3px; display: block; }
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
  .tarjeta.seleccionado { border-color: #3b82f6; }
  /* El hueco donde caerá la tarjeta que se está reordenando */
  .tarjeta.arrastrada { opacity: 0.35; border-style: dashed; border-color: #60a5fa; }

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
  .vacio { color: #52525b; font-size: 10px; text-align: center; margin-top: 10px; }
</style>
