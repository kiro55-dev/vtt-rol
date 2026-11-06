<!-- src/routes/componentes/panel-izquierdo/BancoEntidades.svelte -->
<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { partida } from "$lib/partida.svelte";
  import { miniaturas } from "$lib/miniaturas.svelte";
  import { importarImagen } from "$lib/imagenes";

  const EXTENSIONES = ["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif", "svg"];
  const ES_IMAGEN = new RegExp(`\\.(${EXTENSIONES.join("|")})$`, "i");

  // Filtros
  let busqueda = $state("");
  let filtroZona = $state<"todos" | "actual">("todos");
  let filtroTipo = $state<"todos" | "Enemigo" | "NPC" | "Jugador">("todos");

  // Control de qué filas están visibles en el scroll para descargar imágenes
  let visibles = $state<Record<number, boolean>>({});

  // Filtrado combinado: Nombre + Filtro de Tipo + Filtro de Zona original
  let entidadesFiltradas = $derived(
    partida.banco.filter((e) => {
      const coincideNombre = e.nombre.toLowerCase().includes(busqueda.toLowerCase());
      const coincideZona = filtroZona === "todos" || e.zona_id === partida.zonaId;
      const coincideTipo = filtroTipo === "todos" || e.tipo === filtroTipo;
      return coincideNombre && coincideZona && coincideTipo;
    })
  );

  // IntersectionObserver: desmonta la imagen si la fila sale del scroll
  function vigilarVisibilidad(node: HTMLElement, id: number) {
    const root = node.closest(".sub-entidades");
    const observer = new IntersectionObserver(
      ([entry]) => {
        visibles[id] = entry.isIntersecting;
        if (entry.isIntersecting) {
          const ent = partida.banco.find((b) => b.id === id);
          if (ent?.foto_url) {
            miniaturas.asegurarFicha(ent.foto_url);
          }
        }
      },
      { root, rootMargin: "40px" }
    );

    observer.observe(node);

    return {
      destroy() {
        delete visibles[id];
        observer.disconnect();
      }
    };
  }

  // Drag & drop de imagen desde el explorador del SO
  onMount(() => {
    let unlisten: (() => void) | null = null;
    let desmontado = false;

    try {
      getCurrentWebview()
        .onDragDropEvent(async (event) => {
          if (event.payload.type !== "drop") return;
          const ruta = event.payload.paths?.[0];
          if (!ruta || !ES_IMAGEN.test(ruta)) return;

          const escala = window.devicePixelRatio || 1;
          const { x, y } = event.payload.position;
          const el = document.elementFromPoint(x / escala, y / escala);

          if (el?.closest(".banco")) {
            const fotoUrl = await importarImagen(ruta);
            partida.abrirEditorEntidad(null, fotoUrl);
          }
        })
        .then((fn) => (desmontado ? fn() : (unlisten = fn)))
        .catch((err) => console.warn("dragDropEvent no disponible:", err));
    } catch (err) {
      console.warn("dragDropEvent no disponible:", err);
    }

    return () => {
      desmontado = true;
      unlisten?.();
    };
  });
</script>

<div class="banco">
  <!-- Zona de búsqueda y filtros -->
  <div class="sub-busqueda">
    <div class="fila-cabecera">
      <span class="titulo-mini">BANCO DE FICHAS</span>
      <button type="button" class="btn-anadir" onclick={() => partida.abrirEditorEntidad(null)}>
        + Añadir Ficha
      </button>
    </div>

    <input type="text" placeholder="Buscar ficha..." bind:value={busqueda} class="input-buscar" />

    <!-- Filtros rápidos unificados en dos filas -->
    <div class="filtros-rapidos">
      <!-- Fila superior: Todas las zonas vs Zona actual -->
      <div class="fila-superior">
        <button
          type="button"
          class:activo={filtroZona === "todos"}
          onclick={() => (filtroZona = "todos")}
        >
          Todas
        </button>
        <button
          type="button"
          class:activo={filtroZona === "actual"}
          onclick={() => (filtroZona = "actual")}
        >
          Zona actual
        </button>
      </div>

      <!-- Fila inferior: Todos / Enemigos / NPCs -->
      <div class="fila-inferior">
        <button
          type="button"
          class:activo={filtroTipo === "todos"}
          onclick={() => (filtroTipo = "todos")}
        >
          Todos
        </button>
        <button
          type="button"
          class:activo={filtroTipo === "Enemigo"}
          onclick={() => (filtroTipo = "Enemigo")}
        >
          Enemigos
        </button>
        <button
          type="button"
          class:activo={filtroTipo === "NPC"}
          onclick={() => (filtroTipo = "NPC")}
        >
          NPCs
        </button>
        <button
          type="button"
          class:activo={filtroTipo === "Jugador"}
          onclick={() => (filtroTipo = "Jugador")}
        >
          Jugadores
        </button>
      </div>
    </div>
  </div>

  <!-- Contenedor con scroll para las fichas -->
  <div class="sub-entidades">
    {#each entidadesFiltradas as item (item.id)}
      {@const estaEnPantalla = visibles[item.id] ?? false}
      {@const mini = estaEnPantalla && item.foto_url ? miniaturas.deFicha(item.foto_url) : ""}

      <button
        type="button"
        class="fila-entidad"
        use:vigilarVisibilidad={item.id}
        onclick={() => partida.abrirEditorEntidad(item)}
        title="Clic para editar ficha"
      >
        {#if item.tipo === "NPC"}
          <div class="contenedor-rombo">
            {#if estaEnPantalla && mini}
              <img src={mini} alt="" class="img-rombo" draggable="false" />
            {/if}
          </div>
        {:else if item.tipo === "Enemigo"}
          <div class="contenedor-cuadrado">
            {#if estaEnPantalla && mini}
              <img src={mini} alt="" class="img-cuadrado" draggable="false" />
            {/if}
          </div>
        {:else}
          <div class="contenedor-circulo">
            {#if estaEnPantalla && mini}
              <img src={mini} alt="" class="img-circulo" draggable="false" />
            {/if}
          </div>
        {/if}

        <span class="texto-recortado">{item.nombre}</span>
      </button>
    {/each}

    {#if entidadesFiltradas.length === 0}
      <span class="sin-fichas">No hay fichas</span>
    {/if}
  </div>
</div>

<style>
  .banco {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 5px;
    overflow: hidden;
  }
  .sub-busqueda { display: flex; flex-direction: column; gap: 4px; }
  .fila-cabecera { display: flex; justify-content: space-between; align-items: center; }
  .titulo-mini { font-size: 10px; font-weight: bold; color: #71717a; }
  .btn-anadir {
    background: #27272a;
    border: 1px dashed #52525b;
    color: #60a5fa;
    border-radius: 3px;
    font-size: 10px;
    font-weight: bold;
    padding: 2px 6px;
    cursor: pointer;
  }
  .btn-anadir:hover { background: #3f3f46; color: white; }

  .input-buscar {
    background: #09090b;
    border: 1px solid #3f3f46;
    color: white;
    padding: 3px 5px;
    border-radius: 4px;
    font-size: 11px;
  }

  /* Reset completo para los botones de filtro rápido */
  .filtros-rapidos {
    display: flex;
    flex-direction: column;
    gap: 3px;
    width: 100%;
  }

  .fila-superior,
  .fila-inferior {
    display: flex;
    gap: 3px;
    width: 100%;
  }
  /* Estilo base compartido para todos los botones de ambas filas */
  .filtros-rapidos button {
    flex: 1;
    background: #1c1c1f;
    border: 1px solid #27272a;
    color: #a1a1aa;
    font-size: 10px;
    padding: 3px 0;
    border-radius: 3px;
    cursor: pointer;
    text-align: center;
    line-height: normal;
  }

  .filtros-rapidos button:hover {
    background: #27272a;
    color: #f4f4f5;
  }

  /* Estado seleccionado (activo) */
  .filtros-rapidos button.activo {
    background: #27272a;
    color: #ffffff;
    border-color: #3f3f46;
    font-weight: bold;
  }

  .sub-entidades {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding-right: 2px;
  }

  /* Fila de ficha en el banco */
  .fila-entidad {
    display: flex;
    align-items: center;
    gap: 8px;
    background: #1f1f23;
    padding: 4px 6px;
    border-radius: 4px;
    cursor: pointer;
    border: 1px solid transparent;
    width: 100%;
    color: #f4f4f5;
    font: inherit;
    text-align: left;
    height: 32px;
    box-sizing: border-box;
    flex-shrink: 0;
  }
  .fila-entidad:hover {
    background: #27272a;
    border-color: #3f3f46;
  }

  /* Rombo azul para NPC */
  .contenedor-rombo {
    width: 18px;
    height: 18px;
    transform: rotate(45deg);
    overflow: hidden;
    border: 2px solid #3b82f6;
    border-radius: 2px;
    background: #1e3a8a;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    margin-left: 2px;
    margin-right: 2px;
  }
  .img-rombo {
    width: 142%;
    height: 142%;
    object-fit: cover;
    transform: rotate(-45deg);
    pointer-events: none;
  }

  /* Cuadrado rojo para Enemigos */
  .contenedor-cuadrado {
    width: 18px;
    height: 18px;
    border: 2px solid #ef4444;
    border-radius: 3px;
    overflow: hidden;
    background: #7f1d1d;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    margin-left: 2px;
    margin-right: 2px;
  }
  .img-cuadrado {
    width: 100%;
    height: 100%;
    object-fit: cover;
    pointer-events: none;
  }
  .contenedor-circulo {
    width: 18px;
    height: 18px;
    border: 2px solid #078c09;
    border-radius: 50%;
    overflow: hidden;
    background: #23e612;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    margin-left: 2px;
    margin-right: 2px;
  }
  .img-circulo {
    width: 100%;
    height: 100%;
    object-fit: cover;
    pointer-events: none;
  }
  .texto-recortado {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11px;
  }
  .sin-fichas {
    color: #52525b;
    font-size: 10px;
    text-align: center;
    margin-top: 10px;
  }
</style>