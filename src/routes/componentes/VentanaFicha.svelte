<!-- src/routes/componentes/VentanaFicha.svelte -->
<script lang="ts">
  import { onMount, tick } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { partida, type Entidad } from "$lib/partida.svelte";
  import { importarImagen, urlDeImagen } from "$lib/imagenes";


  interface ArchivoAdjunto {
    ruta: string;
    entidad_archivo: Entidad;
  }

   // Función para abrir el archivo con la app predeterminada del sistema
  async function abrirEnSistema(rutaRelativa: string) {
    try {
      await invoke("abrir_archivo_en_sistema", { rutaRelativa });
    } catch (err) {
      console.error("Error al abrir archivo con el visor del sistema:", err);
    }
  }

  let { entidad }: { entidad: Entidad } = $props();

  // Evita doble clic en Guardar y muestra errores de validación locales
  let guardando = $state(false);
  let errorLocal = $state("");
  const error = $derived(errorLocal || partida.errorEditor);

  // Registro de archivos creados/importados durante esta sesión de edición
  // La foto con la que nace un borrador (ficha arrastrada desde una imagen) también es de esta sesión
  // svelte-ignore state_referenced_locally
  let archivosCreadosEnSesion = $state<string[]>(
    partida.editorEsBorrador && entidad.foto_url ? [entidad.foto_url] : []
  );

  const EXTENSIONES_IMAGEN = ["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif", "svg"];
  const ES_IMAGEN = new RegExp(`\\.(${EXTENSIONES_IMAGEN.join("|")})$`, "i");

  let pos = $state({ x: 260, y: 70 });
  let ventana = $state<HTMLDivElement | null>(null);
  let pestanaActiva = $state<number>(0);

  // Textos cargados en memoria
  let textosArchivos = $state<Record<string, string>>({});
  let textoActual = $state("");

  // Control de edición de nombre de archivo con doble clic
  let editandoTabIndice = $state<number | null>(null);
  let textoEdicionTab = $state("");

  // Control de scroll y flechas
  let barraTabs = $state<HTMLDivElement | null>(null);
  let puedeScrollIzq = $state(false);
  let puedeScrollDer = $state(false);

  // Gestión de cuerpo X por Y
  let cuerpoX = $state(1);
  let cuerpoY = $state(1);

  // Inicializar dimensiones del cuerpo a partir de la entidad
  if (typeof entidad.cuerpo === "object" && entidad.cuerpo !== null && "Grande" in entidad.cuerpo) {
    cuerpoX = entidad.cuerpo.Grande[0] ?? 1;
    cuerpoY = entidad.cuerpo.Grande[1] ?? 1;
  } else {
    cuerpoX = 1;
    cuerpoY = 1;
  }

  function actualizarCuerpo() {
    cuerpoX = Math.max(1, Math.round(Number(cuerpoX) || 1));
    cuerpoY = Math.max(1, Math.round(Number(cuerpoY) || 1));
    if (cuerpoX === 1 && cuerpoY === 1) {
      entidad.cuerpo = "Pequenio";
    } else {
      entidad.cuerpo = { Grande: [cuerpoX, cuerpoY] };
    }
  }

  // --- Handlers de Rueda de Ratón (al pasar por encima) ---
  function alRuedaVida(e: WheelEvent) {
    e.preventDefault();
    const delta = e.deltaY < 0 ? 1 : -1;
    entidad.vida_max = Math.max(0, (Number(entidad.vida_max) || 0) + delta);
  }

  function alRuedaArmadura(e: WheelEvent) {
    e.preventDefault();
    const delta = e.deltaY < 0 ? 1 : -1;
    entidad.armadura = Math.max(0, (Number(entidad.armadura) || 0) + delta);
  }

  function alRuedaVelocidad(e: WheelEvent) {
    e.preventDefault();
    const delta = e.deltaY < 0 ? 1 : -1;
    entidad.velocidad = Math.max(0, (Number(entidad.velocidad) || 0) + delta);
  }

  function alRuedaRango(e: WheelEvent) {
    e.preventDefault();
    const delta = e.deltaY < 0 ? 1 : -1;
    entidad.rango_arma = Math.max(0, (Number(entidad.rango_arma) || 0) + delta);
  }

  function alRuedaCuerpoX(e: WheelEvent) {
    e.preventDefault();
    const delta = e.deltaY < 0 ? 1 : -1;
    cuerpoX = Math.max(1, cuerpoX + delta);
    actualizarCuerpo();
  }

  function alRuedaCuerpoY(e: WheelEvent) {
    e.preventDefault();
    const delta = e.deltaY < 0 ? 1 : -1;
    cuerpoY = Math.max(1, cuerpoY + delta);
    actualizarCuerpo();
  }

  function nombreLimpio(ruta: string | null | undefined): string {
    if (!ruta) return "Documento_1.txt";
    return ruta.split(/[/\\]/).pop() ?? ruta;
  }

  function esTexto(ruta: string | null | undefined): boolean {
    if (!ruta) return false;
    return ruta.toLowerCase().endsWith(".txt");
  }

  function comprobarScroll() {
    if (!barraTabs) return;
    puedeScrollIzq = barraTabs.scrollLeft > 2;
    puedeScrollDer = barraTabs.scrollLeft + barraTabs.clientWidth < barraTabs.scrollWidth - 2;
  }

  function scrollIzquierda() {
    barraTabs?.scrollBy({ left: -100, behavior: "smooth" });
  }

  function scrollDerecha() {
    barraTabs?.scrollBy({ left: 100, behavior: "smooth" });
  }

  async function cargarContenidoPestana(indice: number) {
    if (entidad.archivos_adjuntos.length === 0) {
      textoActual = "";
      return;
    }
    pestanaActiva = indice;
    const ruta = entidad.archivos_adjuntos[indice];
    if (!ruta || !esTexto(ruta)) {
      textoActual = "";
      return;
    }

    if (textosArchivos[ruta] !== undefined) {
      textoActual = textosArchivos[ruta];
    } else {
      try {
        const contenido = await invoke<string>("leer_archivo_texto", { rutaRelativa: ruta });
        textosArchivos[ruta] = contenido;
        // Si mientras se leía se cambió de pestaña, no pisar el texto de la nueva
        if (entidad.archivos_adjuntos[pestanaActiva] === ruta) textoActual = contenido;
      } catch (err) {
        console.error("Error al leer texto:", err);
        textoActual = "";
      }
    }
  }

  function alModificarTexto(e: Event) {
    const val = (e.target as HTMLTextAreaElement).value;
    textoActual = val;
    const ruta = entidad.archivos_adjuntos[pestanaActiva];
    if (ruta) {
      textosArchivos[ruta] = val;
    }
  }

  // Crear .txt manual
  async function crearNuevoTxt() {
    try {
      const nuevoAdjunto = await invoke<ArchivoAdjunto>("crear_documento_texto", { entidad: $state.snapshot(entidad) });
      entidad.archivos_adjuntos.push(nuevoAdjunto.ruta);
      archivosCreadosEnSesion.push(nuevoAdjunto.ruta); // <- Registrado como creado en sesión
      textosArchivos[nuevoAdjunto.ruta] = "";
      await tick();
      cargarContenidoPestana(entidad.archivos_adjuntos.length - 1);
      comprobarScroll();
    } catch (err) {
      errorLocal = `No se pudo crear el documento: ${err}`;
    }
  }

  // Cancelar: Rust descarta los archivos creados en esta sesión (y, si la ficha era nueva, su
  // carpeta y su id reservado). Nunca toca archivos que la ficha guardada ya tenía.
  async function cancelarYDescartar() {
    if (guardando) return;
    await partida.cancelarEditorEntidad($state.snapshot(archivosCreadosEnSesion));
  }

  async function borrarFicha() {
    if (guardando) return;
    const sesion = $state.snapshot(archivosCreadosEnSesion);
    await partida.borrarEntidad(entidad);
    // Lo que se creó en esta sesión y la ficha guardada nunca llegó a referenciar
    if (!partida.entidadEnEdicion && sesion.length > 0) {
      try {
        await invoke("descartar_archivos_borrador", { rutas: sesion });
      } catch (err) {
        console.error("Error al descartar archivos:", err);
      }
    }
  }

  function iniciarRenombrado(indice: number, ruta: string) {
    editandoTabIndice = indice;
    textoEdicionTab = nombreLimpio(ruta);
  }

  async function confirmarRenombrado(indice: number) {
    if (editandoTabIndice === null) return;
    editandoTabIndice = null;

    const rutaVieja = entidad.archivos_adjuntos[indice];
    if (!rutaVieja) return;

    const nuevoNombre = textoEdicionTab.trim();
    if (!nuevoNombre || nuevoNombre === nombreLimpio(rutaVieja)) return;

    try {
      const adjuntoActualizado = await invoke<ArchivoAdjunto>("renombrar_archivo_adjunto", {
        adjunto: { ruta: rutaVieja, entidad_archivo: $state.snapshot(entidad) },
        nuevoNombre
      });

      entidad.archivos_adjuntos[indice] = adjuntoActualizado.ruta;

      // Si el original pertenece a la ficha ya guardada, Rust lo COPIÓ (no se pierde si se
      // cancela): la copia es un archivo nuevo de esta sesión. Si lo habíamos creado en esta
      // sesión, se movió: sustituimos su entrada.
      const k = archivosCreadosEnSesion.indexOf(rutaVieja);
      if (k >= 0) archivosCreadosEnSesion[k] = adjuntoActualizado.ruta;
      else archivosCreadosEnSesion.push(adjuntoActualizado.ruta);

      if (textosArchivos[rutaVieja] !== undefined) {
        textosArchivos[adjuntoActualizado.ruta] = textosArchivos[rutaVieja];
        delete textosArchivos[rutaVieja];
      }
    } catch (err) {
      errorLocal = `No se pudo renombrar: ${err}`;
    }
  }

  function desasociarAdjunto(indice: number) {
    const [eliminado] = entidad.archivos_adjuntos.splice(indice, 1);
    if (eliminado) delete textosArchivos[eliminado];
    // Si se quita una pestaña anterior a la activa, la activa sigue siendo la misma ficha
    if (indice < pestanaActiva) pestanaActiva -= 1;
    pestanaActiva = Math.max(0, Math.min(pestanaActiva, entidad.archivos_adjuntos.length - 1));
    cargarContenidoPestana(pestanaActiva);
    comprobarScroll();
  }

  // Número entero dentro del rango de i16 que usa Rust
  function entero(v: unknown, nombre: string, min = 0): number {
    const n = typeof v === "number" ? v : Number(v);
    if (v === null || v === "" || !Number.isInteger(n)) throw new Error(`${nombre}: debe ser un número entero`);
    if (n < min) throw new Error(`${nombre}: no puede ser menor que ${min}`);
    if (n > 32767) throw new Error(`${nombre}: no puede pasar de 32767`);
    return n;
  }

  function validar() {
    if (!entidad.nombre?.trim()) throw new Error("La ficha necesita un nombre");
    entidad.vida_max = entero(entidad.vida_max, "Vida");
    entidad.armadura = entero(entidad.armadura, "Armadura");
    entidad.velocidad = entero(entidad.velocidad, "Velocidad");
    entidad.rango_arma = entero(entidad.rango_arma, "Rango");
    cuerpoX = entero(cuerpoX, "Cuerpo X", 1);
    cuerpoY = entero(cuerpoY, "Cuerpo Y", 1);
    if (cuerpoX > 255 || cuerpoY > 255) throw new Error("El cuerpo no puede pasar de 255");
    actualizarCuerpo();
  }

  // Guardar: valida, escribe los textos y la ficha. Si algo falla, la ventana sigue abierta
  // con el motivo a la vista y NO se descarta nada.
  async function guardarTodo() {
    if (guardando) return;
    guardando = true;
    errorLocal = "";
    try {
      validar();

      for (const [ruta, contenido] of Object.entries(textosArchivos)) {
        if (!entidad.archivos_adjuntos.includes(ruta)) continue; // pestaña desasociada
        await invoke("guardar_archivo_texto", {
          adjunto: { ruta, entidad_archivo: $state.snapshot(entidad) },
          contenido
        });
      }

      const sesion = $state.snapshot(archivosCreadosEnSesion);
      if (await partida.guardarEntidad(entidad)) {
        // Fotos/archivos de esta sesión que al final no se usaron (cambió de foto, desasoció
        // un archivo...). Rust nunca borra lo que la partida referencia.
        if (sesion.length > 0) {
          try {
            await invoke("descartar_archivos_borrador", { rutas: sesion });
          } catch (err) {
            console.error("Error al limpiar archivos sobrantes:", err);
          }
        }
      }
    } catch (err) {
      errorLocal = err instanceof Error ? err.message : String(err);
    } finally {
      guardando = false;
    }
  }

   function alTeclearVentana(e: KeyboardEvent) {
    if (e.key === "Enter") {
      if ((e.target as HTMLElement)?.tagName === "TEXTAREA" || editandoTabIndice !== null) return;
      e.preventDefault();
      guardarTodo();
    } else if (e.key === "Escape") {
      cancelarYDescartar();
    }
  }

  function iniciarArrastre(e: PointerEvent) {
    if (e.button !== 0) return;
    const inicioX = e.clientX;
    const inicioY = e.clientY;
    const origenX = pos.x;
    const origenY = pos.y;

    const alMover = (ev: PointerEvent) => {
      pos.x = Math.max(10, Math.min(window.innerWidth - 500, origenX + (ev.clientX - inicioX)));
      pos.y = Math.max(10, Math.min(window.innerHeight - 300, origenY + (ev.clientY - inicioY)));
    };
    const alSoltar = () => {
      window.removeEventListener("pointermove", alMover);
      window.removeEventListener("pointerup", alSoltar);
    };
    window.addEventListener("pointermove", alMover);
    window.addEventListener("pointerup", alSoltar);
  }

  async function seleccionarFoto(e: MouseEvent) {
    e.preventDefault();
    const ruta = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Imágenes", extensions: EXTENSIONES_IMAGEN }]
    });
    if (typeof ruta === "string") {
      await ponerFoto(ruta);
    }
  }

  async function ponerFoto(origen: string) {
    try {
      const nueva = await importarImagen(origen);
      entidad.foto_url = nueva;
      if (!archivosCreadosEnSesion.includes(nueva)) archivosCreadosEnSesion.push(nueva);
    } catch (err) {
      errorLocal = `No se pudo importar la imagen: ${err}`;
    }
  }

  onMount(() => {
    ventana?.focus(); // para que Enter/Escape funcionen sin tocar la ventana con el ratón
    if (entidad.archivos_adjuntos.length > 0) {
      cargarContenidoPestana(0);
    }

    const resizeObserver = new ResizeObserver(() => comprobarScroll());
    if (barraTabs) resizeObserver.observe(barraTabs);

    let unlisten: (() => void) | null = null;
    let desmontado = false;

    try {
      getCurrentWebview()
        .onDragDropEvent(async (event) => {
          if (event.payload.type !== "drop") return;
          const rutas = event.payload.paths ?? [];
          if (rutas.length === 0) return;

          const escala = window.devicePixelRatio || 1;
          const { x, y } = event.payload.position;
          const el = document.elementFromPoint(x / escala, y / escala);

          // 1. Arrastre a foto
          if (el?.closest(".caja-foto")) {
            const rutaImg = rutas.find((r) => ES_IMAGEN.test(r));
            if (rutaImg) {
              await ponerFoto(rutaImg);
            }
            return;
          }

          // 2. Arrastre a archivos
          if (el?.closest(".zona-archivos")) {
            for (const r of rutas) {
              try {
                const adjunto = await invoke<ArchivoAdjunto>("importar_archivo", {
                  adjunto: { ruta: "", entidad_archivo: $state.snapshot(entidad) },
                  origen: r
                });
                entidad.archivos_adjuntos.push(adjunto.ruta);
                archivosCreadosEnSesion.push(adjunto.ruta);
              } catch (err) {
                errorLocal = `No se pudo importar «${r}»: ${err}`;
              }
            }
            await tick();
            cargarContenidoPestana(entidad.archivos_adjuntos.length - 1);
            comprobarScroll();
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
      resizeObserver.disconnect();
    };
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  class="ventana-flotante"
  style="left: {pos.x}px; top: {pos.y}px;"
  role="dialog"
  aria-label="Ficha"
  tabindex="-1"
  onkeydown={alTeclearVentana}
  bind:this={ventana}
>
  <!-- Cabecera movible -->
  <div class="cabecera-flotante" role="presentation" onpointerdown={iniciarArrastre}>
    <span class="icono-drag">⋮⋮</span>
    <span class="titulo-ventana">
      {partida.editorEsBorrador ? "Nueva Ficha" : `Ficha: ${entidad.nombre}`}
    </span>
    <button type="button" class="btn-cerrar" onclick={cancelarYDescartar}>✕</button>
  </div>

  <div class="cuerpo-ficha">
    <div class="fila-superior">
      <!-- Caja de Foto -->
      <div
        class="caja-foto"
        role="button"
        tabindex="0"
        oncontextmenu={seleccionarFoto}
        title="Clic derecho para elegir imagen o arrastra una aquí"
      >
        {#if entidad.foto_url}
          <img src={urlDeImagen(entidad.foto_url)} alt="Foto ficha" class="img-preview" draggable="false" />
        {:else}
          <div class="foto-placeholder">
            <span>Foto</span>
            <small>(público siempre)</small>
          </div>
        {/if}
      </div>

      <!-- Datos de la ficha -->
      <div class="columna-campos">
        <div class="campo-fila">
          <label for="f-nombre">Nombre:</label>
          <input id="f-nombre" type="text" class="input-texto" bind:value={entidad.nombre} />
        </div>

        <!-- Fila de Vida y Armadura con Rueda de Ratón -->
        <div class="campo-fila fila-stats">
          <label title="Rueda del ratón para subir/bajar">
            Vida max:
            <input
              type="number"
              class="input-num"
              bind:value={entidad.vida_max}
              onwheel={alRuedaVida}
            />
          </label>
          <label title="Rueda del ratón para subir/bajar">
            Armadura:
            <input
              type="number"
              class="input-num"
              bind:value={entidad.armadura}
              onwheel={alRuedaArmadura}
            />
          </label>
        </div>

        <!-- Fila con los checks de visibilidad -->
        <div class="campo-fila fila-checks">
          <label class="check-publico">
            <span>Hacer pública la vida</span>
            <input type="checkbox" bind:checked={entidad.vida_es_publica} />
          </label>
          <label class="check-publico">
            <span>Hacer pública la armadura</span>
            <input type="checkbox" bind:checked={entidad.armadura_es_publica} />
          </label>
        </div>

        <!-- Fila de Velocidad y Rango de Arma con Rueda de Ratón -->
        <div class="campo-fila fila-stats">
          <label title="Rueda del ratón para subir/bajar">
            Velocidad:
            <input
              type="number"
              class="input-num"
              bind:value={entidad.velocidad}
              onwheel={alRuedaVelocidad}
            />
            <span class="sufijo">m</span>
          </label>
          <label title="Rueda del ratón para subir/bajar">
            Rango arma:
            <input
              type="number"
              class="input-num"
              bind:value={entidad.rango_arma}
              onwheel={alRuedaRango}
            />
            <span class="sufijo">m</span>
          </label>
        </div>

        <!-- Fila de Cuerpo (X por Y) con Rueda de Ratón independiente -->
        <div class="campo-fila fila-cuerpo">
          <label>Cuerpo:</label>
          <div class="cuerpo-inputs" title="Rueda del ratón sobre cada número para ajustar dimensiones (metros/casillas)">
            <input
              type="number"
              class="input-num input-cuerpo"
              min="1"
              bind:value={cuerpoX}
              oninput={actualizarCuerpo}
              onwheel={alRuedaCuerpoX}
            />
            <span class="separador-x">×</span>
            <input
              type="number"
              class="input-num input-cuerpo"
              min="1"
              bind:value={cuerpoY}
              oninput={actualizarCuerpo}
              onwheel={alRuedaCuerpoY}
            />
            <span class="sufijo">m</span>
          </div>
        </div>

        <!-- Tipo y Único -->
        <div class="campo-fila">
          <label for="f-tipo">Tipo:</label>
          <select id="f-tipo" class="select-general" bind:value={entidad.tipo}>
            <option value="Enemigo">Enemigo</option>
            <option value="NPC">NPC</option>
            <option value="Jugador">Jugador</option>
          </select>

          <label class="check-publico" style="margin-left: 8px;">
            <input type="checkbox" bind:checked={entidad.es_unico} />
            <span title="Si es único solo existirá una instancia en el mundo">Único</span>
          </label>
        </div>

        <!-- Zona asociada -->
        <div class="campo-fila">
          <label for="f-zona">Zona asociada:</label>
          <select id="f-zona" class="select-general" bind:value={entidad.zona_id}>
            {#each partida.zonasOrdenadas as z}
              <option value={z.id}>{z.nombre}</option>
            {/each}
          </select>
        </div>
      </div>
    </div>

    <!-- Zona de Archivos Adjuntos -->
    <div class="zona-archivos">
      <div class="barra-pestanas-wrapper">
        {#if puedeScrollIzq}
          <button type="button" class="btn-flecha-scroll" onclick={scrollIzquierda} title="Desplazar a la izquierda">
            ◀
          </button>
        {/if}

        <div class="contenedor-tabs-scroll" bind:this={barraTabs} onscroll={comprobarScroll}>
          {#each entidad.archivos_adjuntos as archivo, i}
            {#if archivo}
              <button
                type="button"
                class="btn-pestana"
                class:activa={pestanaActiva === i}
                onclick={() => cargarContenidoPestana(i)}
                title="Doble clic para renombrar archivo"
              >
                {#if editandoTabIndice === i}
                  <input
                    type="text"
                    class="input-renombrar-tab"
                    bind:value={textoEdicionTab}
                    onblur={() => confirmarRenombrado(i)}
                    onkeydown={(e) => {
                      if (e.key === "Enter") {
                        e.stopPropagation();
                        confirmarRenombrado(i);
                      }
                      if (e.key === "Escape") {
                        e.stopPropagation();
                        editandoTabIndice = null;
                      }
                    }}
                    onclick={(e) => e.stopPropagation()}
                    autofocus
                  />
                {:else}
                  <span
                    class="nombre-tab"
                    ondblclick={(e) => {
                      e.stopPropagation();
                      iniciarRenombrado(i, archivo);
                    }}
                  >
                    {nombreLimpio(archivo)}
                  </span>
                {/if}

                <span
                  class="btn-cerrar-tab"
                  role="button"
                  tabindex="0"
                  onclick={(e) => { e.stopPropagation(); desasociarAdjunto(i); }}
                >×</span>
              </button>
            {/if}
          {/each}
        </div>

        {#if puedeScrollDer}
          <button type="button" class="btn-flecha-scroll" onclick={scrollDerecha} title="Desplazar a la derecha">
            ▶
          </button>
        {/if}

        <button type="button" class="btn-anadir-adjunto" onclick={crearNuevoTxt} title="Crear documento .txt">+</button>
      </div>

      <!-- Visor de contenido -->
      <div class="visor-archivo">
        {#if entidad.archivos_adjuntos.length === 0}
          <div class="archivo-placeholder">
            <span>Sin archivos adjuntos. Arrastra archivos aquí o pulsa + para crear uno.</span>
          </div>
        {:else if esTexto(entidad.archivos_adjuntos[pestanaActiva])}
          <textarea
            class="editor-texto"
            placeholder="Notas privadas..."
            value={textoActual}
            oninput={alModificarTexto}
          ></textarea>
        {:else}
          {@const rutaArchivo = entidad.archivos_adjuntos[pestanaActiva]}
          <div class="archivo-placeholder">
            <span class="nombre-archivo-centro">{nombreLimpio(rutaArchivo)}</span>
            <small>(Archivo no editable como texto)</small>
            {#if rutaArchivo}
              <button
                type="button"
                class="btn-abrir-sistema"
                onclick={() => abrirEnSistema(rutaArchivo)}
              >
                Abrir en el visor del sistema
              </button>
            {/if}
          </div>
        {/if}
      </div>
    </div>

    {#if error}
      <p class="error-ficha" role="alert">{error}</p>
    {/if}

    <div class="pie-acciones">
      {#if !partida.editorEsBorrador}
        <button type="button" class="btn-borrar" onclick={borrarFicha} disabled={guardando}>
          Borrar
        </button>
      {/if}
      <div class="botones-derecha">
        <button type="button" class="btn-cancelar" onclick={cancelarYDescartar}>
          Cancelar
        </button>
        <button type="button" class="btn-guardar" onclick={guardarTodo} disabled={guardando}>
          Guardar
        </button>
      </div>
    </div>
  </div>
</div>

<style>
  .ventana-flotante {
    position: fixed;
    width: 480px;
    background: #18181b;
    border: 1px solid #3f3f46;
    border-radius: 8px;
    box-shadow: 0 14px 35px rgba(0, 0, 0, 0.75);
    z-index: 1000;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    user-select: none;
    font-size: 11px;
    color: #f4f4f5;
  }
  .cabecera-flotante {
    background: #27272a;
    padding: 6px 10px;
    display: flex;
    align-items: center;
    gap: 6px;
    cursor: grab;
    border-bottom: 1px solid #3f3f46;
  }
  .cabecera-flotante:active { cursor: grabbing; }
  .icono-drag { color: #71717a; font-size: 12px; }
  .titulo-ventana { font-weight: bold; flex: 1; }
  .btn-cerrar { background: none; border: none; color: #a1a1aa; cursor: pointer; font-size: 13px; }
  .btn-cerrar:hover { color: white; }

  .cuerpo-ficha { padding: 10px; display: flex; flex-direction: column; gap: 8px; }
  .fila-superior { display: flex; gap: 10px; align-items: flex-start; }

  .caja-foto {
    width: 110px;
    height: 110px;
    background: #09090b;
    border: 1px dashed #52525b;
    border-radius: 6px;
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    cursor: pointer;
  }
  .caja-foto:hover { border-color: #3b82f6; }
  .img-preview { width: 100%; height: 100%; object-fit: cover; }
  .foto-placeholder { text-align: center; color: #71717a; font-size: 11px; }
  .foto-placeholder small { display: block; font-size: 8px; margin-top: 2px; }

  .columna-campos { flex: 1; display: flex; flex-direction: column; gap: 4px; }
  .campo-fila { display: flex; align-items: center; gap: 6px; }
  .campo-fila label { color: #a1a1aa; font-size: 10px; display: flex; align-items: center; gap: 4px; }
  .fila-stats { display: flex; gap: 10px; }

  .fila-checks {
    display: flex;
    gap: 12px;
    padding: 1px 0;
  }
  .check-publico {
    display: flex;
    align-items: center;
    gap: 4px;
    cursor: pointer;
    font-size: 10px;
    color: #d4d4d8;
  }
  .check-publico input { cursor: pointer; accent-color: #3b82f6; margin: 0; }

  .input-texto {
    flex: 1; background: #09090b; border: 1px solid #3f3f46; color: white;
    padding: 3px 6px; border-radius: 4px; font-size: 11px;
  }
  .input-num {
    width: 44px; background: #09090b; border: 1px solid #3f3f46; color: white;
    padding: 2px 4px; border-radius: 4px; font-size: 11px;
  }
  .sufijo { color: #71717a; font-size: 10px; }

  /* Fila de cuerpo X x Y */
  .fila-cuerpo { display: flex; align-items: center; gap: 6px; }
  .cuerpo-inputs { display: flex; align-items: center; gap: 4px; }
  .input-cuerpo { width: 34px; text-align: center; }
  .separador-x { color: #71717a; font-weight: bold; font-size: 11px; }

  .select-general {
    background: #09090b; border: 1px solid #3f3f46; color: white;
    padding: 2px 4px; border-radius: 4px; font-size: 10px;
  }

  .zona-archivos {
    border: 1px solid #27272a;
    border-radius: 6px;
    background: #111113;
    overflow: hidden;
  }
  .barra-pestanas-wrapper {
    display: flex;
    background: #1f1f23;
    border-bottom: 1px solid #27272a;
    align-items: center;
    height: 28px;
  }
  .contenedor-tabs-scroll {
    display: flex;
    flex: 1;
    overflow-x: auto;
    scrollbar-width: none;
    height: 100%;
  }
  .contenedor-tabs-scroll::-webkit-scrollbar { display: none; }

  .btn-pestana {
    display: flex;
    align-items: center;
    gap: 5px;
    background: none;
    border: none;
    color: #a1a1aa;
    padding: 0 8px;
    font-size: 10px;
    cursor: pointer;
    border-right: 1px solid #27272a;
    white-space: nowrap;
    height: 100%;
    flex-shrink: 0;
  }
  .btn-pestana.activa { background: #111113; color: white; font-weight: bold; }

  .nombre-tab {
    cursor: pointer;
  }

  .input-renombrar-tab {
    background: #09090b;
    border: 1px solid #3b82f6;
    color: white;
    font-size: 10px;
    padding: 1px 3px;
    border-radius: 2px;
    outline: none;
    width: 90px;
  }

  .btn-cerrar-tab {
    color: #71717a;
    font-size: 11px;
    padding: 0 2px;
  }
  .btn-cerrar-tab:hover { color: #ef4444; }

  .btn-flecha-scroll {
    background: #27272a;
    border: none;
    color: #e4e4e7;
    font-size: 8px;
    padding: 0 6px;
    height: 100%;
    cursor: pointer;
    flex-shrink: 0;
    z-index: 2;
  }
  .btn-flecha-scroll:hover { background: #3f3f46; color: white; }

  .btn-anadir-adjunto {
    background: none; border: none; color: #a1a1aa; padding: 0 8px;
    cursor: pointer; font-size: 13px; font-weight: bold; height: 100%; flex-shrink: 0;
  }
  .btn-anadir-adjunto:hover { color: white; background: #27272a; }

  .visor-archivo { height: 100px; padding: 4px; }
  .editor-texto {
    width: 100%; height: 100%; background: transparent; border: none;
    color: #e4e4e7; font-family: inherit; font-size: 11px; resize: none; outline: none;
    box-sizing: border-box;
  }
  .archivo-placeholder {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    color: #a1a1aa;
    gap: 4px;
    text-align: center;
    font-size: 11px;
    user-select: none;
  }

  .error-ficha { margin: 0 0 6px; color: #f87171; font-size: 12px; }
  .pie-acciones { display: flex; justify-content: space-between; align-items: center; }
  .botones-derecha { display: flex; gap: 6px; margin-left: auto; }
  .btn-cancelar, .btn-guardar, .btn-borrar {
    border: none; padding: 5px 12px; border-radius: 4px; font-size: 11px; font-weight: 600; cursor: pointer;
  }
  .btn-cancelar { background: #27272a; color: #d4d4d8; }
  .btn-guardar { background: #2563eb; color: white; }
  .btn-borrar { background: #7f1d1d; color: #fca5a5; }
  .btn-cancelar:hover { background: #3f3f46; }
  .btn-guardar:hover { background: #1d4ed8; }
  .btn-borrar:hover { background: #991b1b; }

  .btn-abrir-sistema:hover {
    background: #3b82f6;
    border-color: #60a5fa;
    color: white;
  }
  .nombre-archivo-centro {
    color: #f4f4f5;
    font-weight: 500;
  }

  .archivo-placeholder small {
    color: #71717a;
    font-size: 10px;
  }

  .btn-abrir-sistema {
    margin-top: 4px;
    background: #27272a;
    border: 1px solid #3f3f46;
    color: #60a5fa;
    padding: 4px 10px;
    border-radius: 4px;
    font-size: 10px;
    font-weight: 500;
    cursor: pointer;
    transition: background 0.15s ease, color 0.15s ease;
  }

  .btn-abrir-sistema:hover {
    background: #3f3f46;
    color: white;
  }
</style>