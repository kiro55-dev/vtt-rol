<!-- src/routes/componentes/VentanaSala.svelte -->
<!-- Ventana flotante «Configuración de sala» (mismo estilo que la de la ficha). Abrir o cerrar la sala,
     el enlace para los jugadores, la contraseña, el mapa que ven y quién está conectado. Arrastrar a una
     persona sobre una ficha (banco, «Fichas activas» o tablero) le da el control de esa ficha.
     La lógica está en Rust (src-tauri/src/red/) y en $lib/sala. -->
<script lang="ts" module>
  import { ayuda } from "$lib/ajustesVista.svelte";
  let ultimaPosicion = { x: 300, y: 80 };
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import { sala } from "$lib/sala.svelte";
  import { partida } from "$lib/partida.svelte";
  import { arrastreJugador } from "$lib/arrastreJugador.svelte";
  import { moverVentana } from "$lib/moverVentana";

  let ventana = $state<HTMLDivElement | null>(null);
  let pos = $state({ ...ultimaPosicion });
  $effect(() => {
    ultimaPosicion = { x: pos.x, y: pos.y };
  });

  let error = $state("");
  // Jugador cuya baja de la partida está pendiente de confirmar
  let quitando = $state<number | null>(null);
  /** La URL que se acaba de copiar (para poner «¡Copiado!» en su botón). */
  let copiado = $state<string | null>(null);
  let contrasena = $state("");
  let confirmandoEnlace = $state(false);
  let ocupado = $state(false);

  // «Cómo se publica»: lo que se está editando (se guarda con el botón; solo con la sala cerrada)
  // Tres formas: «Peer to peer» (directo, con un respaldo), «Túnel» (todos por un servidor: sin conexión directa) y «Red
  // local». En las dos primeras se elige Cloudflare o tu propia dirección.
  let forma = $state<"p2p" | "tunel" | "lan">("p2p");
  let respaldo = $state<"Cloudflare" | "Propio">("Cloudflare");
  const modo = $derived<"Cloudflare" | "Propio" | "Lan">(forma === "lan" ? "Lan" : respaldo);
  let urlPropia = $state("");
  let puertoPropio = $state(8080);
  let configGuardada = $state("");
  const configEditada = $derived(
    JSON.stringify({
      respaldo:
        modo === "Cloudflare"
          ? { tipo: "Cloudflare" }
          : modo === "Propio"
            ? { tipo: "Propio", url: urlPropia.trim(), puerto: Number(puertoPropio) }
            : { tipo: "Lan", puerto: Number(puertoPropio) },
      solo_respaldo: forma === "tunel"
    })
  );
  const hayCambios = $derived(configEditada !== configGuardada);

  async function cargarConfig() {
    const c = await sala.leerConfig();
    forma = c.respaldo.tipo === "Lan" ? "lan" : c.solo_respaldo ? "tunel" : "p2p";
    if (c.respaldo.tipo !== "Lan") respaldo = c.respaldo.tipo;
    if (c.respaldo.tipo === "Propio") {
      urlPropia = c.respaldo.url;
      puertoPropio = c.respaldo.puerto;
    } else if (c.respaldo.tipo === "Lan") {
      puertoPropio = c.respaldo.puerto;
    }
    configGuardada = JSON.stringify(c);
  }

  /** «Abrir sala» abre con lo que está elegido ahora: si cambió, se guarda antes (sin botón aparte). */
  function abrirConLoElegido() {
    void hacer(async () => {
      if (hayCambios) {
        await sala.guardarConfig(JSON.parse(configEditada)); // Rust comprueba la dirección y el puerto
        await cargarConfig();
      }
      await sala.abrir();
    });
  }

  const fase = $derived(sala.estado.fase);
  const nombreFicha = (id: number) => partida.banco.find((e) => e.id === id)?.nombre ?? `Ficha ${id}`;

  onMount(() => {
    void sala.recargar();
    void cargarConfig().catch((e) => (error = String(e)));
  });

  /** Ejecuta una acción mostrando su error, sin dejar pulsar otra a la vez. */
  async function hacer(accion: () => Promise<unknown>) {
    if (ocupado) return;
    ocupado = true;
    error = "";
    try {
      await accion();
    } catch (err) {
      error = String(err);
    } finally {
      ocupado = false;
    }
  }

  async function copiar(url: string) {
    if (!(await sala.copiarEnlace(url))) return;
    copiado = url;
    setTimeout(() => (copiado = copiado === url ? null : copiado), 1500);
  }

  function guardarContrasena() {
    void hacer(async () => {
      await sala.ponerContrasena(contrasena);
      contrasena = "";
    });
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  class="ventana-flotante"
  style="left: {pos.x}px; top: {pos.y}px;"
  role="dialog"
  aria-label="Configuración de sala"
  tabindex="-1"
  data-conserva-foco
  onkeydown={(e) => e.key === "Escape" && (sala.ventanaAbierta = false)}
  bind:this={ventana}
>
  <div class="cabecera" role="presentation" onpointerdown={(e) => moverVentana(e, pos, ventana)}>
    <span class="icono-drag">⋮⋮</span>
    <span class="titulo">Configuración de sala</span>
    <button type="button" class="btn-cerrar" aria-label="Cerrar" onclick={() => (sala.ventanaAbierta = false)}>✕</button>
  </div>

  <div class="cuerpo">
    <!-- Estado y abrir/cerrar -->
    <div class="fila">
      {#if fase.tipo === "Cerrada"}
        <span class="estado cerrada">● Sala cerrada</span>
        <button type="button" class="btn-principal" disabled={ocupado} onclick={abrirConLoElegido}>Abrir sala</button>
      {:else if fase.tipo === "Preparando"}
        <span class="estado preparando">● {fase.paso}{fase.progreso !== null ? ` ${Math.round(fase.progreso * 100)} %` : ""}</span>
        <button type="button" class="btn-secundario" onclick={() => hacer(() => sala.cerrar())}>Cancelar</button>
      {:else}
        <span class="estado abierta">● Sala abierta</span>
        <button type="button" class="btn-peligro" disabled={ocupado} onclick={() => hacer(() => sala.cerrar())}>Cerrar sala</button>
      {/if}
    </div>
    {#if fase.tipo === "Preparando" && fase.progreso !== null}
      <div class="barra"><div class="relleno" style:width="{Math.round(fase.progreso * 100)}%"></div></div>
    {/if}

    <!-- Cómo se publica en internet (de este equipo, no de la partida) -->
    <div class="seccion">
      <span class="etiqueta">Cómo llegan los jugadores</span>
      {#if fase.tipo === "Cerrada"}
        {#snippet eleccion(titulo: string)}
          <div class="subopciones">
            <span class="nota">{titulo}</span>
            <label class="opcion">
              <input type="radio" name="respaldo" value="Cloudflare" bind:group={respaldo} />
              <span>Cloudflare <span class="nota">(automático, sin configurar nada)</span></span>
            </label>
            <label class="opcion">
              <input type="radio" name="respaldo" value="Propio" bind:group={respaldo} />
              <span>Tu propia dirección <span class="nota">(tu dominio, Tailscale funnel, ngrok, tu proxy https…)</span></span>
            </label>
          </div>
        {/snippet}
        <label class="opcion">
          <input type="radio" name="forma" value="p2p" bind:group={forma} />
          <span>Peer to peer <span class="nota">(directo con cada jugador, con un respaldo por si su red no lo permite)</span></span>
        </label>
        {#if forma === "p2p"}
          {@render eleccion("Respaldo:")}
          <span class="nota">Con la conexión directa, tú y cada jugador veis la IP pública del otro. Si no quieres que vean la tuya, usa «Túnel».</span>
        {/if}
        <label class="opcion">
          <input type="radio" name="forma" value="tunel" bind:group={forma} />
          <span>Túnel <span class="nota">(todos entran por un servidor, sin conexión directa: nadie ve tu IP)</span></span>
        </label>
        {#if forma === "tunel"}
          {@render eleccion("Servidor:")}
          {#if respaldo === "Propio"}
            <span class="nota">Con tu propia dirección, los jugadores solo ven esa dirección, no tu IP.</span>
          {/if}
        {/if}
        <label class="opcion">
          <input type="radio" name="forma" value="lan" bind:group={forma} />
          <span>Red local o VPN <span class="nota">(como «abrir en LAN»: Radmin VPN, Hamachi, ZeroTier, tu wifi…)</span></span>
        </label>
        {#if modo === "Propio"}
          <div class="fila">
            <input class="input-texto" placeholder="https://mi.dominio.es" maxlength="260" bind:value={urlPropia} />
            <input class="input-texto input-puerto" type="number" min="1024" max="65535" title="Puerto de este equipo" bind:value={puertoPropio} />
          </div>
          <span class="nota">
            Haz que esa dirección (https) lleve a <code>http://127.0.0.1:{puertoPropio}</code> de este equipo. La app escucha
            ahí al abrir la sala y sirve ella misma la página de los jugadores.
          </span>
        {:else if modo === "Lan"}
          <div class="fila">
            <span class="nota">Puerto</span>
            <input class="input-texto input-puerto" type="number" min="1024" max="65535" title="Puerto de este equipo" bind:value={puertoPropio} />
          </div>
          <span class="nota aviso">
            Como «abrir en LAN»: los jugadores entran con <code>http://&lt;tu IP en esa red&gt;:{puertoPropio}</code>, sin
            más (al abrir verás un enlace por cada red). <b>Cualquiera que llegue a esa IP y puerto puede entrar</b>, y va
            sin cifrar: úsalo en redes de confianza y, si no lo son, pon contraseña. La primera vez Windows te pedirá
            permiso en el cortafuegos.
          </span>
        {/if}
        {#if modo !== "Cloudflare" && !sala.estado.pagina_incluida}
          <span class="nota aviso">Esta versión de la app no lleva la página de los jugadores: solo funcionará el túnel de Cloudflare.</span>
        {/if}
      {:else}
        <span class="nota">
          {sala.estado.respaldo.tipo === "Propio"
            ? `Por tu dirección propia (${sala.estado.respaldo.url}, puerto ${sala.estado.respaldo.puerto}).`
            : sala.estado.respaldo.tipo === "Lan"
              ? `En la red local, puerto ${sala.estado.respaldo.puerto} (sin cifrar).`
              : "Por un túnel de Cloudflare."}
          Cierra la sala para cambiarlo.
        </span>
        {#if sala.estado.solo_respaldo && sala.estado.respaldo.tipo !== "Lan"}
          <span class="nota">Por túnel: no se ofrece la conexión directa.</span>
        {/if}
        {#if sala.estado.senal}
          <span class="nota">Intercambio automático de la conexión directa: relés conectados {sala.estado.senal.reles_conectados} de {sala.estado.senal.reles_total}.</span>
        {/if}
      {/if}
    </div>

    <!-- Enlace -->
    <div class="seccion">
      <span class="etiqueta">Enlace para los jugadores</span>
      {#if sala.estado.enlaces.length > 0}
        {#each sala.estado.enlaces as e (e.url)}
          {#if sala.estado.enlaces.length > 1}<span class="nota">{e.etiqueta}</span>{/if}
          <div class="fila">
            <input class="input-texto" readonly value={e.url} onfocus={(ev) => ev.currentTarget.select()} />
            <button type="button" class="btn-secundario" onclick={() => copiar(e.url)}>{copiado === e.url ? "¡Copiado!" : "Copiar"}</button>
          </div>
        {/each}
        <span class="nota">
          {sala.estado.respaldo.tipo === "Cloudflare"
            ? "Cambia cada vez que abres la sala."
            : sala.estado.respaldo.tipo === "Lan"
              ? "Pasa a cada jugador el de la red en la que estéis juntos (p. ej. el de Radmin VPN)."
              : "Con tu dirección propia no cambia al volver a abrir la sala."}
          Los que ya entraron no repiten la contraseña.
        </span>
      {:else}
        <span class="nota">Abre la sala para obtenerlo.</span>
      {/if}
      {#if sala.estado.respaldo.tipo === "Lan"}
        <!-- En red local el enlace no lleva llave: no hay nada que cambiar -->
      {:else if confirmandoEnlace}
        <div class="confirmar">
          <span>El enlace actual dejará de servir, los conectados saldrán y todos tendrán que volver a poner la contraseña.</span>
          <div class="fila">
            <button type="button" class="btn-peligro" onclick={() => hacer(async () => { await sala.regenerarEnlace(); confirmandoEnlace = false; })}>Generar enlace nuevo</button>
            <button type="button" class="btn-secundario" onclick={() => (confirmandoEnlace = false)}>No</button>
          </div>
        </div>
      {:else}
        <button type="button" class="btn-enlace" onclick={() => (confirmandoEnlace = true)}>Generar enlace nuevo (por si se ha filtrado)</button>
      {/if}
    </div>

    <!-- Contraseña -->
    <div class="seccion">
      <span class="etiqueta">
        Contraseña {sala.estado.pide_contrasena ? "(puesta)" : "(ninguna: entra cualquiera con el enlace)"}
      </span>
      <div class="fila">
        <input
          class="input-texto"
          type="password"
          placeholder={sala.estado.pide_contrasena ? "Nueva contraseña" : "Poner contraseña"}
          maxlength="128"
          bind:value={contrasena}
          onkeydown={(e) => e.key === "Enter" && contrasena && guardarContrasena()}
        />
        <button type="button" class="btn-secundario" disabled={ocupado || !contrasena} onclick={guardarContrasena}>Guardar</button>
        {#if sala.estado.pide_contrasena}
          <button type="button" class="btn-secundario" disabled={ocupado} onclick={() => hacer(() => sala.ponerContrasena(null))}>Quitar</button>
        {/if}
      </div>
    </div>

    <!-- Mapas que ven los jugadores: se eligen con la casilla de cada mapa en la lista de mapas -->
    <div class="seccion">
      <span class="etiqueta">Mapas que ven los jugadores</span>
      {#if sala.estado.mapas_compartidos.length > 0}
        <div class="compartidos">
          {#each sala.estado.mapas_compartidos as m (m.id)}
            <span class="compartido">{m.nombre}</span>
          {/each}
        </div>
      {:else}
        <span class="nota">Ninguno.</span>
      {/if}
      <span class="nota">Marca la casilla de arriba a la derecha de cada mapa para compartirlo (y desmárcala para dejar de hacerlo). Puedes abrir y preparar otros mapas: solo ven los marcados.</span>
    </div>

    <!-- Jugadores -->
    <div class="seccion">
      <span class="etiqueta">Jugadores de esta partida · arrastra uno sobre una ficha para dársela</span>
      <div class="jugadores">
        {#each sala.jugadores as j (j.id)}
          <div class="jugador">
            <button
              type="button"
              class="nombre-jugador"
              class:conectado={j.conectado}
              title={ayuda("Arrastra sobre una ficha (banco, «Fichas activas» o tablero) para que la controle")}
              onpointerdown={(e) => arrastreJugador.preparar(e, j)}
            >
              <span class="punto">●</span> {j.nombre}
            </button>
            {#if j.conectado && j.via}
              <span class="via" class:directo={j.via === "directo"} title={ayuda(j.via === "directo" ? "Conectado directo (peer to peer)" : "Conectado por el respaldo (túnel, dirección propia o red local)")}>
                {sala.estado.respaldo.tipo === "Lan" ? "Red local" : j.via === "directo" ? "Directo" : "Respaldo"}
              </span>
            {/if}
            <div class="fichas">
              {#each j.fichas as id (id)}
                <span class="ficha">
                  {nombreFicha(id)}
                  <button type="button" title={ayuda("Quitarle esta ficha")} onclick={() => hacer(() => sala.quitar(j.id, id))}>✕</button>
                </span>
              {/each}
            </div>
            {#if j.conectado}
              <button type="button" class="btn-expulsar" title={ayuda("Lo saca de la sala; para volver tendrá que poner la contraseña")} onclick={() => hacer(() => sala.expulsar(j.id))}>Expulsar</button>
            {/if}
            {#if quitando === j.id}
              <span class="nota">¿Quitarlo de la partida? Sus fichas se quedan sin dueño.</span>
              <button type="button" class="btn-expulsar" onclick={() => { quitando = null; void hacer(() => sala.eliminar(j.id)); }}>Sí, quitar</button>
              <button type="button" class="btn-no" onclick={() => (quitando = null)}>No</button>
            {:else}
              <button type="button" class="btn-expulsar" title={ayuda("Lo borra de esta partida (también lo saca de la sala). Sus fichas no se borran: se quedan sin dueño")} onclick={() => (quitando = j.id)}>Quitar de la partida</button>
            {/if}
          </div>
        {:else}
          <span class="nota">Aún no ha entrado nadie. Los que entren se quedan guardados en esta partida.</span>
        {/each}
      </div>
    </div>

    {#if error || sala.estado.error}
      <p class="error" role="alert">{error || sala.estado.error}</p>
    {/if}
  </div>
</div>

<!-- La persona que se está arrastrando -->
{#if arrastreJugador.actual}
  {@const a = arrastreJugador.actual}
  <div class="fantasma" style="left: {a.x + 12}px; top: {a.y + 8}px;">
    {a.jugador.nombre}{a.idEntidad !== null ? ` → ${nombreFicha(a.idEntidad)}` : ""}
  </div>
{/if}

<style>
  .ventana-flotante {
    position: fixed; z-index: 1000; width: 420px; max-height: calc(100vh - 40px);
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

  .cuerpo { padding: 10px; display: flex; flex-direction: column; gap: 10px; overflow-y: auto; }
  .seccion { display: flex; flex-direction: column; gap: 4px; }
  .fila { display: flex; align-items: center; gap: 6px; }
  .etiqueta { color: #a1a1aa; font-size: 10px; }
  .nota { color: #71717a; font-size: 10px; }

  .estado { flex: 1; font-weight: bold; }
  .estado.cerrada { color: #71717a; }
  .estado.preparando { color: #facc15; }
  .estado.abierta { color: #22c55e; }
  .barra { height: 4px; background: #27272a; border-radius: 2px; overflow: hidden; }
  .relleno { height: 100%; background: #facc15; }

  .input-texto {
    flex: 1; min-width: 0; padding: 3px 6px; border-radius: 4px;
    background: #09090b; border: 1px solid #3f3f46; color: white; font-size: 11px;
  }
  .btn-principal, .btn-secundario, .btn-peligro {
    border: none; padding: 4px 10px; border-radius: 4px; font-size: 11px; font-weight: 600; cursor: pointer; white-space: nowrap;
  }
  .btn-principal { background: #2563eb; color: white; }
  .btn-principal:hover { background: #1d4ed8; }
  .btn-secundario { background: #27272a; color: #d4d4d8; }
  .btn-secundario:hover { background: #3f3f46; }
  .btn-peligro { background: #7f1d1d; color: #fca5a5; }
  .btn-peligro:hover { background: #991b1b; }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  .btn-enlace { align-self: flex-start; background: none; border: none; color: #60a5fa; font-size: 10px; cursor: pointer; padding: 0; }
  .btn-enlace:hover { text-decoration: underline; }
  .confirmar { display: flex; flex-direction: column; gap: 4px; padding: 6px; border: 1px solid #7f1d1d; border-radius: 4px; color: #fca5a5; font-size: 10px; }

  .compartidos { display: flex; flex-wrap: wrap; gap: 3px; }
  .compartido { background: #14532d; color: #bbf7d0; border-radius: 3px; padding: 1px 6px; font-size: 10px; }
  .jugadores { display: flex; flex-direction: column; gap: 4px; }
  .jugador { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; background: #1f1f23; border-radius: 4px; padding: 4px 6px; }
  .nombre-jugador {
    background: #27272a; border: 1px solid #3f3f46; color: #f4f4f5; border-radius: 12px; padding: 2px 8px;
    font-size: 11px; cursor: grab; touch-action: none;
  }
  .nombre-jugador .punto { color: #52525b; }
  .nombre-jugador.conectado .punto { color: #22c55e; }
  .via { font-size: 10px; padding: 1px 6px; border-radius: 8px; background: #422006; color: #fbbf24; }
  .via.directo { background: #052e16; color: #4ade80; }
  .fichas { display: flex; gap: 3px; flex-wrap: wrap; flex: 1; }
  .ficha { background: #14532d; color: #bbf7d0; border-radius: 3px; padding: 1px 4px; font-size: 10px; display: flex; gap: 3px; align-items: center; }
  .ficha button { background: none; border: none; color: #86efac; cursor: pointer; font-size: 9px; padding: 0; }
  .btn-no { background: none; border: none; color: #a1a1aa; font-size: 10px; cursor: pointer; }
  .btn-expulsar { background: none; border: none; color: #f87171; font-size: 10px; cursor: pointer; }
  .error { margin: 0; color: #f87171; font-size: 12px; }
  .opcion { display: flex; align-items: center; gap: 6px; font-size: 12px; cursor: pointer; }
  .nota.aviso { color: #fbbf24; }
  .subopciones { display: flex; flex-direction: column; gap: 3px; margin-left: 22px; }
  .input-texto.input-puerto { width: 80px; flex: none; }

  .fantasma {
    position: fixed; z-index: 2000; pointer-events: none;
    background: #14532d; color: #bbf7d0; border: 1px solid #22c55e; border-radius: 12px; padding: 2px 8px; font-size: 11px;
  }
</style>
