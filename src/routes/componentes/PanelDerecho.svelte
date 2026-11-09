<!-- src/routes/componentes/PanelDerecho.svelte -->
<!-- Solo la estructura del panel: cada parte vive en su componente y lee el estado de los stores. -->
<script>
  import FichasActivas from "./panel-derecho/FichasActivas.svelte";
  import ParametrosFicha from "./panel-derecho/ParametrosFicha.svelte";
  import TiradasChat from "./panel-derecho/TiradasChat.svelte";
</script>

<aside class="panel-derecho">
  <div class="sub-fichas-activas"><FichasActivas /></div>
  <div class="sub-parametros"><ParametrosFicha /></div>
  <!-- El chat es de todos: las tiradas del jugador las hace el host y llegan con el nombre de quien tira -->
  <div class="sub-tiradas"><TiradasChat /></div>
</aside>

<style>
  .panel-derecho {
    width: clamp(200px, 16vw, 250px); /* Borde más estrecho */
    height: 100%;
    background: #141416;
    border-left: 1px solid #27272a;
    display: flex;
    flex-direction: column;
    padding: 6px;
    gap: 6px;
    box-sizing: border-box;
    font-size: 12px;
  }
  .sub-fichas-activas { flex: 1.2; display: flex; flex-direction: column; overflow: hidden; }
  .sub-parametros { height: 190px; display: flex; flex-direction: column; }
  .sub-tiradas { height: 200px; display: flex; flex-direction: column; }
</style>
