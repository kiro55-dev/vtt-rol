<!-- src/routes/componentes/ventana-ficha/CajaFoto.svelte -->
<!-- Foto de la ficha. Clic derecho abre el selector; soltar una imagen encima lo gestiona
     VentanaFicha (busca la clase `.caja-foto` bajo el cursor). -->
<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { EXTENSIONES_IMAGEN, urlDeImagen } from "$lib/imagenes";

  let { foto, onElegir }: { foto: string | null; onElegir: (origen: string) => void } = $props();

  async function elegir(e: MouseEvent) {
    e.preventDefault();
    const ruta = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Imágenes", extensions: EXTENSIONES_IMAGEN.ficha }]
    });
    if (typeof ruta === "string") onElegir(ruta);
  }
</script>

<div
  class="caja-foto"
  role="button"
  tabindex="0"
  oncontextmenu={elegir}
  title="Clic derecho para elegir imagen o arrastra una aquí"
>
  {#if foto}
    <img src={urlDeImagen(foto)} alt="Foto ficha" draggable="false" />
  {:else}
    <div class="placeholder">
      <span>Foto</span>
      <small>(público siempre)</small>
    </div>
  {/if}
</div>

<style>
  .caja-foto {
    width: 110px; height: 110px; flex-shrink: 0;
    background: #09090b; border: 1px dashed #52525b; border-radius: 6px; overflow: hidden;
    display: flex; align-items: center; justify-content: center; cursor: pointer;
  }
  .caja-foto:hover { border-color: #3b82f6; }
  img { width: 100%; height: 100%; object-fit: cover; }
  .placeholder { text-align: center; color: #71717a; font-size: 11px; }
  .placeholder small { display: block; font-size: 8px; margin-top: 2px; }
</style>
