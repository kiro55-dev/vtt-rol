<!-- src/routes/componentes/ventana-ficha/CajaFoto.svelte -->
<!-- Foto de la ficha. Clic derecho abre el selector; soltar una imagen encima lo gestiona
     VentanaFicha (busca la clase `.caja-foto` bajo el cursor).
     Aquí se encuadra la miniatura de la ficha: arrastrar con el botón izquierdo mueve la foto y la
     rueda la acerca o la aleja. Lo que se ve en la caja es el recorte que guarda Rust (la misma cuenta
     que `recortar` en miniaturas.rs), y esa única miniatura es la que usa todo lo demás. -->
<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { EXTENSIONES_IMAGEN, urlDeImagen } from "$lib/imagenes";
  import { ZOOM_MAX, ZOOM_MIN, tamanoEnCaja, type Encuadre } from "$lib/recorteFoto";

  let {
    foto,
    encuadre = $bindable({ x: 50, y: 50, zoom: ZOOM_MIN }),
    onElegir,
    onCambio
  }: {
    foto: string | null;
    encuadre?: Encuadre;
    onElegir: (origen: string) => void;
    /** El usuario movió o acercó la foto */
    onCambio?: () => void;
  } = $props();

  const LADO = 110; // px de la caja (cuadrada), igual que en el CSS
  const PASO_ZOOM = 1.1;

  let caja = $state<HTMLDivElement | null>(null);
  let natural = $state<{ ancho: number; alto: number } | null>(null);
  let arrastrando = $state(false);
  let agarre: { x: number; y: number; encuadre: Encuadre } | null = null;

  /** Tamaño en px de la foto entera con ese zoom (100 = cubre justo la caja). */
  const tamano = (zoom: number) => (natural ? tamanoEnCaja(natural, LADO, zoom) : { ancho: LADO, alto: LADO });

  const tam = $derived(tamano(encuadre.zoom));
  // Lo que sobra de la foto en cada eje: lo que se puede desplazar
  const sobra = $derived({ x: tam.ancho - LADO, y: tam.alto - LADO });
  const hayFoto = $derived(!!foto && natural !== null);

  const limitar = (n: number, min = 0, max = 100) => Math.max(min, Math.min(max, Math.round(n)));

  async function elegir(e: MouseEvent) {
    e.preventDefault();
    const ruta = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Imágenes", extensions: EXTENSIONES_IMAGEN.ficha }]
    });
    if (typeof ruta === "string") onElegir(ruta);
  }

  function alPulsar(e: PointerEvent) {
    if (e.button !== 0 || !hayFoto) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    agarre = { x: e.clientX, y: e.clientY, encuadre: { ...encuadre } };
    arrastrando = true;
  }

  // La foto sigue al cursor: arrastrarla a la derecha deja ver más de su parte izquierda (encuadre menor)
  function alMover(e: PointerEvent) {
    if (!agarre) return;
    if (sobra.x > 0.5) encuadre.x = limitar(agarre.encuadre.x - ((e.clientX - agarre.x) / sobra.x) * 100);
    if (sobra.y > 0.5) encuadre.y = limitar(agarre.encuadre.y - ((e.clientY - agarre.y) / sobra.y) * 100);
    onCambio?.();
  }

  function alSoltar() {
    agarre = null;
    arrastrando = false;
  }

  // Rueda: acerca o aleja manteniendo quieto el punto de la foto que hay bajo el cursor
  function alRueda(e: WheelEvent) {
    if (!hayFoto || !caja) return;
    e.preventDefault();
    const zoom = limitar(encuadre.zoom * (e.deltaY < 0 ? PASO_ZOOM : 1 / PASO_ZOOM), ZOOM_MIN, ZOOM_MAX);
    if (zoom === encuadre.zoom) return;

    const r = caja.getBoundingClientRect();
    const p = { x: e.clientX - r.left, y: e.clientY - r.top };
    const antes = tamano(encuadre.zoom);
    const despues = tamano(zoom);
    // Punto de la foto (en px, con el zoom de antes) que hay bajo el cursor, y el desplazamiento con el que
    // ese mismo punto sigue bajo el cursor con el zoom nuevo
    const u = { x: ((antes.ancho - LADO) * encuadre.x) / 100 + p.x, y: ((antes.alto - LADO) * encuadre.y) / 100 + p.y };
    const nuevoX = (u.x / antes.ancho) * despues.ancho - p.x;
    const nuevoY = (u.y / antes.alto) * despues.alto - p.y;

    if (despues.ancho - LADO > 0.5) encuadre.x = limitar((nuevoX / (despues.ancho - LADO)) * 100);
    if (despues.alto - LADO > 0.5) encuadre.y = limitar((nuevoY / (despues.alto - LADO)) * 100);
    encuadre.zoom = zoom;
    onCambio?.();
  }

  // La rueda no puede ser pasiva para poder evitar que se desplace la ventana
  $effect(() => {
    const el = caja;
    if (!el) return;
    el.addEventListener("wheel", alRueda, { passive: false });
    return () => el.removeEventListener("wheel", alRueda);
  });
</script>

<div
  class="caja-foto"
  class:movible={hayFoto}
  class:arrastrando
  role="button"
  tabindex="0"
  bind:this={caja}
  oncontextmenu={elegir}
  onpointerdown={alPulsar}
  onpointermove={alMover}
  onpointerup={alSoltar}
  onpointercancel={alSoltar}
  title={hayFoto
    ? "Arrastra para mover la foto · Rueda para acercar o alejar · Clic derecho para cambiar la imagen"
    : "Clic derecho para elegir imagen o arrastra una aquí"}
>
  {#if foto}
    <img
      src={urlDeImagen(foto)}
      alt="Foto ficha"
      draggable="false"
      class:medida={natural !== null}
      style={natural
        ? `width:${tam.ancho}px;height:${tam.alto}px;left:${(-sobra.x * encuadre.x) / 100}px;top:${(-sobra.y * encuadre.y) / 100}px`
        : ""}
      onload={(e) => {
        const img = e.currentTarget as HTMLImageElement;
        natural = { ancho: img.naturalWidth, alto: img.naturalHeight };
      }}
    />
  {:else}
    <div class="placeholder">
      <span>Foto</span>
      <small>(público siempre)</small>
    </div>
  {/if}
</div>

<style>
  .caja-foto {
    position: relative;
    box-sizing: content-box; /* el área de la foto mide justo LADO, sin restar el borde */
    width: 110px; height: 110px; flex-shrink: 0;
    background: #09090b; border: 1px dashed #52525b; border-radius: 6px; overflow: hidden;
    display: flex; align-items: center; justify-content: center; cursor: pointer;
    touch-action: none; user-select: none;
  }
  .caja-foto:hover { border-color: #3b82f6; }
  .caja-foto.movible { cursor: grab; }
  .caja-foto.arrastrando { cursor: grabbing; border-color: #3b82f6; }
  img { width: 100%; height: 100%; object-fit: cover; pointer-events: none; }
  /* Con la medida conocida se coloca a mano (tamaño y posición del recorte) */
  img.medida { position: absolute; max-width: none; object-fit: fill; }
  .placeholder { text-align: center; color: #71717a; font-size: 11px; }
  .placeholder small { display: block; font-size: 8px; margin-top: 2px; }
</style>
