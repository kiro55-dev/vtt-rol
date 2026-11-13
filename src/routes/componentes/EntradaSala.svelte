<!-- src/routes/componentes/EntradaSala.svelte -->
<!-- Nombre (y contraseña, si el host puso una) para entrar en la sala por primera vez. -->
<script lang="ts">
  import { untrack } from "svelte";
  import type { Conexion, EstadoConexion } from "$lib/red/conexion.svelte";

  let { conexion, estado }: { conexion: Conexion; estado: Extract<EstadoConexion, { tipo: "identificate" }> } = $props();

  let nombre = $state("");
  let contrasena = $state("");
  let enviando = $state(false);
  // Al volver a preguntar (contraseña mal...) se puede intentar otra vez
  $effect(() => {
    void estado;
    enviando = false;
  });

  // El nombre escrito ya está registrado y nadie lo usa: se ofrece entrar igualmente (mientras no lo cambie)
  let nombreOfrecido = $state<string | null>(null);
  // Solo cuando llega la respuesta del host (`estado`): lo que se teclee después no la renueva
  $effect(() => {
    const ofrecido = estado.nombreExistente;
    nombreOfrecido = ofrecido ? untrack(() => nombre.trim()) : null;
  });
  const ofrecer = $derived(!!estado.nombreExistente && nombreOfrecido !== null && nombre.trim() === nombreOfrecido);

  // El mismo botón: «Entrar», o «Entrar igualmente» si el nombre ya existe y nadie lo usa
  function entrar(e: SubmitEvent) {
    e.preventDefault();
    if (!nombre.trim() || enviando) return;
    enviando = true;
    conexion.unirse(nombre, contrasena, ofrecer);
  }
</script>

<form class="entrada" onsubmit={entrar}>
  <h1>Entrar en la sala</h1>
  {#if estado.recordado}
    <!-- Ya habías entrado y no te reconoce: o el host cambió el enlace, o este no es su servidor -->
    <p class="cuidado">
      Ya habías entrado en esta sala desde este navegador, pero ahora no te reconoce. Si el host no ha generado
      un enlace nuevo, desconfía de este enlace y no pongas la contraseña.
    </p>
  {/if}
  <label>
    Tu nombre
    <input type="text" maxlength="30" autocomplete="nickname" bind:value={nombre} required />
  </label>
  {#if estado.pideContrasena}
    <label>
      Contraseña de la sala
      <input type="password" maxlength="128" autocomplete="current-password" bind:value={contrasena} />
    </label>
  {/if}
  {#if estado.error}<p class="error" role="alert">{estado.error}</p>{/if}
  {#if ofrecer}
    <!-- Ese nombre ya es de un jugador de la partida y no hay nadie con él: el mismo jugador desde otro dispositivo -->
    <p class="info" role="status">Ya existe un jugador con ese nombre registrado (no se perderán los permisos de las fichas).</p>
  {/if}
  <button type="submit" disabled={enviando || !nombre.trim()}>{enviando ? "Entrando…" : ofrecer ? "Entrar igualmente" : "Entrar"}</button>
  <p class="nota">Solo hace falta la primera vez: este navegador lo recordará.</p>
</form>

<style>
  .entrada {
    max-width: 320px; margin: 10vh auto 0; padding: 20px; display: flex; flex-direction: column; gap: 12px;
    background: #18181b; border: 1px solid #3f3f46; border-radius: 8px;
  }
  h1 { margin: 0; font-size: 18px; }
  label { display: flex; flex-direction: column; gap: 4px; font-size: 12px; color: #a1a1aa; }
  input { padding: 6px 8px; border-radius: 4px; background: #09090b; border: 1px solid #3f3f46; color: white; font-size: 14px; }
  button { background: #2563eb; color: white; border: none; border-radius: 4px; padding: 8px; font-size: 14px; font-weight: 600; cursor: pointer; }
  button:disabled { opacity: 0.5; cursor: not-allowed; }
  .error { margin: 0; color: #f87171; font-size: 13px; }
  .cuidado { margin: 0; color: #fbbf24; font-size: 12px; }
  .info { margin: 0; color: #a1a1aa; font-size: 12px; }
  .nota { margin: 0; color: #71717a; font-size: 11px; }
</style>
