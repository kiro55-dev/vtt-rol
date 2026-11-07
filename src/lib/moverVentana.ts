// src/lib/moverVentana.ts
//
// Mover una ventana flotante arrastrando su cabecera, sin que se salga de la pantalla.
// Se usa como `onpointerdown` de la cabecera; los botones de la cabecera no inician el arrastre.
export function moverVentana(e: PointerEvent, pos: { x: number; y: number }, ventana: HTMLElement | null) {
  if (e.button !== 0 || (e.target as HTMLElement).closest("button")) return;
  const dx = e.clientX - pos.x;
  const dy = e.clientY - pos.y;
  const ancho = ventana?.offsetWidth ?? 480;
  const alto = ventana?.offsetHeight ?? 300;

  const mover = (ev: PointerEvent) => {
    pos.x = Math.max(10, Math.min(window.innerWidth - ancho - 10, ev.clientX - dx));
    pos.y = Math.max(10, Math.min(window.innerHeight - alto - 10, ev.clientY - dy));
  };
  const soltar = () => {
    window.removeEventListener("pointermove", mover);
    window.removeEventListener("pointerup", soltar);
    window.removeEventListener("pointercancel", soltar);
  };
  window.addEventListener("pointermove", mover);
  window.addEventListener("pointerup", soltar);
  window.addEventListener("pointercancel", soltar);
}
