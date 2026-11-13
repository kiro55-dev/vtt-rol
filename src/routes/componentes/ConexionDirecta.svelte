<!-- src/routes/componentes/ConexionDirecta.svelte -->
<!-- La conexión directa con el host (punto a punto) con códigos a mano: se copia el propio código y se le pasa al host,
     y se pega el que devuelve. Mientras la señalización automática no esté (fase 4), es lo que hay. Siempre se puede
     entrar por el respaldo (el túnel, la dirección propia). -->
<script lang="ts">
  import type { Conexion, EstadoConexion } from "$lib/red/conexion.svelte";

  let { conexion, estado }: { conexion: Conexion; estado: Extract<EstadoConexion, { tipo: "directo-a-mano" }> } = $props();

  let respuesta = $state("");
  let copiado = $state(false);

  async function copiar() {
    if (!estado.codigo) return;
    try {
      await navigator.clipboard.writeText(estado.codigo);
      copiado = true;
      setTimeout(() => (copiado = false), 1500);
    } catch {
      // Sin permiso para el portapapeles: se puede seleccionar el texto a mano
    }
  }

  function conectar(e: SubmitEvent) {
    e.preventDefault();
    if (!respuesta.trim() || estado.conectando) return;
    void conexion.usarRespuesta(respuesta);
  }
</script>

<form class="directa" onsubmit={conectar}>
  <h1>Conexión directa</h1>
  <p class="nota">
    Punto a punto con el host, sin pasar por ningún servidor. Hay que pasarse dos códigos (por Discord, WhatsApp…).
  </p>

  <label>
    1. Copia este código y pásaselo al host
    <textarea readonly rows="4" value={estado.codigo ?? "Preparando…"} onfocus={(e) => e.currentTarget.select()}></textarea>
  </label>
  <button type="button" class="secundario" disabled={!estado.codigo} onclick={copiar}>{copiado ? "¡Copiado!" : "Copiar mi código"}</button>

  <label>
    2. Pega aquí el código que te devuelve
    <textarea rows="4" bind:value={respuesta} placeholder="Código del host" disabled={!estado.codigo || estado.conectando}></textarea>
  </label>
  {#if estado.error}<p class="error" role="alert">{estado.error}</p>{/if}
  <button type="submit" disabled={!estado.codigo || estado.conectando || !respuesta.trim()}>
    {estado.conectando ? "Conectando…" : "Conectar"}
  </button>

  <hr />
  <p class="nota">¿No hay manera? Entra por el camino de siempre (a través del servidor del host).</p>
  <button type="button" class="secundario" onclick={() => conexion.conectarPorRespaldo()}>Entrar por el respaldo</button>
</form>

<style>
  .directa {
    max-width: 420px; margin: 6vh auto 0; padding: 20px; display: flex; flex-direction: column; gap: 10px;
    background: #18181b; border: 1px solid #3f3f46; border-radius: 8px;
  }
  h1 { margin: 0; font-size: 18px; }
  label { display: flex; flex-direction: column; gap: 4px; font-size: 12px; color: #a1a1aa; }
  textarea {
    padding: 6px 8px; border-radius: 4px; background: #09090b; border: 1px solid #3f3f46; color: white;
    font: 11px monospace; word-break: break-all; resize: vertical;
  }
  button { background: #2563eb; color: white; border: none; border-radius: 4px; padding: 8px; font-size: 14px; font-weight: 600; cursor: pointer; }
  button.secundario { background: #3f3f46; font-weight: 500; }
  button:disabled { opacity: 0.5; cursor: not-allowed; }
  hr { border: none; border-top: 1px solid #3f3f46; margin: 4px 0; }
  .error { margin: 0; color: #f87171; font-size: 13px; }
  .nota { margin: 0; color: #71717a; font-size: 11px; }
</style>
