<!-- src/routes/componentes/comunes/VisorImagen.svelte -->
<!-- La foto de una ficha a resolución completa, leída del archivo original (no de la miniatura), como una
     ventanita flotante: su esquina superior derecha queda en el punto donde se hizo doble clic y no tapa el
     resto. Pulsar en cualquier otro sitio (o Escape) la cierra; esa pulsación sigue su curso normal. El <img>
     solo existe mientras está abierta y se vacía su `src` al cerrar, para que el webview suelte la imagen. -->
<script lang="ts">
  import { visorImagen } from "$lib/visorImagen.svelte";
  import { urlDeImagen } from "$lib/imagenes";

  const MARGEN = 8; // px que se deja libres respecto al borde de la ventana
  const ANCHO_MIN = 240; // si el puntero está muy a la izquierda, tampoco se aplasta la imagen

  let anchoVentana = $state(0);
  let altoVentana = $state(0);
  let alto = $state(0); // alto con el que se dibuja (para no salirse por abajo)

  // Esquina superior derecha en el puntero. Hacia la izquierda y hacia abajo hasta donde quepa en la ventana.
  const derecha = $derived(Math.max(0, anchoVentana - Math.max(visorImagen.x, ANCHO_MIN)));
  const anchoMax = $derived(Math.max(ANCHO_MIN, Math.min(visorImagen.x, anchoVentana) - MARGEN));
  const altoMax = $derived(Math.max(100, altoVentana - 2 * MARGEN));
  const arriba = $derived(Math.max(MARGEN, Math.min(visorImagen.y, altoVentana - alto - MARGEN)));

  // Al cerrar (o cambiar de imagen), se suelta la que estaba cargada
  function liberar(img: HTMLImageElement) {
    return {
      destroy() {
        img.removeAttribute("src");
        img.src = "";
      }
    };
  }

  // Cualquier pulsación fuera de la ventanita la cierra (sin impedir lo que esa pulsación haga)
  function alPulsar(e: PointerEvent) {
    if (visorImagen.ruta && !(e.target as Element | null)?.closest?.(".visor")) visorImagen.cerrar();
  }
</script>

<svelte:window
  bind:innerWidth={anchoVentana}
  bind:innerHeight={altoVentana}
  onpointerdowncapture={alPulsar}
  onkeydown={(e) => e.key === "Escape" && visorImagen.ruta && visorImagen.cerrar()}
/>

{#if visorImagen.ruta}
  <div
    class="visor"
    data-conserva-foco
    style:right="{derecha}px"
    style:top="{arriba}px"
    oncontextmenu={(e) => e.preventDefault()}
    role="img"
    aria-label="Foto de la ficha"
  >
    {#key visorImagen.ruta}
      <img
        use:liberar
        bind:clientHeight={alto}
        src={urlDeImagen(visorImagen.ruta)}
        alt=""
        draggable="false"
        style:max-width="{anchoMax}px"
        style:max-height="{altoMax}px"
      />
    {/key}
  </div>
{/if}

<style>
  .visor {
    position: fixed;
    z-index: 2000;
    border: 2px solid #52525b;
    border-radius: 6px;
    background: #18181b;
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.55);
    overflow: hidden;
    line-height: 0;
  }
  /* Tamaño real; solo se reduce si no cabe en la ventana */
  img {
    display: block;
    object-fit: contain;
    cursor: default;
  }
</style>
