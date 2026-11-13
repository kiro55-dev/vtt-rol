// src/lib/moverConFlecha.ts
//
// Mover una ficha con la flecha del botón derecho: con la flecha dibujada desde una ficha, Enter pone esa ficha en la
// casilla final de la flecha (lo mismo que arrastrarla hasta allí, con los mismos permisos). Funciones puras, con tests;
// el tablero (`Tablero.svelte`) las usa.
import type { Casilla, Destino, Instancia } from "$lib/partida.svelte";

/**
 * Adónde va la ficha de origen de la flecha: se mueve LO MISMO que la flecha, de modo que la casilla de la ficha que se
 * pulsó para empezar (la primera del recorrido) acaba justo en la ÚLTIMA casilla, y el resto de su cuerpo queda alrededor
 * como estaba (si se cogió una de 3×3 por su esquina de abajo a la derecha, esa esquina cae donde termina la flecha y el
 * resto queda arriba a la izquierda). El destino es la casilla de su origen (el centro de su cuerpo,
 * `Instancia.coordenadas`) desplazada igual, y Rust la encaja dentro del mapa si hace falta. `null` si no hay nada que mover: no hay flecha de al menos dos casillas, la
 * ficha no existe, no está puesta en una casilla de ESTE mapa, o quien pulsa no puede moverla (`controla`: el host,
 * siempre; un jugador, solo las suyas; el host lo vuelve a comprobar).
 */
export function destinoDeLaFlecha(p: {
  recorrido: readonly Casilla[];
  instancia: Instancia | undefined;
  idMapa: number | null;
  controla: (i: Instancia) => boolean;
}): Destino | null {
  const { recorrido, instancia, idMapa } = p;
  if (recorrido.length < 2 || idMapa === null || !instancia) return null;
  if (instancia.id_mapa !== idMapa || typeof instancia.coordenadas !== "object" || !("Casilla" in instancia.coordenadas)) return null;
  if (!p.controla(instancia)) return null;
  const [salida, fin] = [recorrido[0], recorrido[recorrido.length - 1]];
  const origen = instancia.coordenadas.Casilla;
  return { id_mapa: idMapa, casilla: { x: origen.x + (fin.x - salida.x), y: origen.y + (fin.y - salida.y) } };
}

/** Lo que hace falta saber de una pulsación de tecla para decidir si el Enter es del tablero. */
export interface TeclaEnter {
  key: string;
  ctrlKey?: boolean;
  altKey?: boolean;
  metaKey?: boolean;
  shiftKey?: boolean;
  repeat?: boolean;
  /** El elemento que tenía el foco (el `target` de un `KeyboardEvent`, o algo con `closest`). */
  target?: EventTarget | { closest?: (selector: string) => unknown } | null;
}

/** Elementos donde el Enter es suyo (escribir, aceptar un formulario, pulsar el botón que tiene el foco...). */
const ENTER_DE_OTROS = "input, textarea, select, button, a, summary, [contenteditable='true']";

/** ¿Es un Enter «suelto» que no pertenece a un campo, un botón ni un enlace? Solo entonces mueve la ficha. */
export function enterEsDelTablero(e: TeclaEnter): boolean {
  if (e.key !== "Enter" || e.repeat) return false;
  if (e.ctrlKey || e.altKey || e.metaKey || e.shiftKey) return false;
  return !(e.target as { closest?: (selector: string) => unknown } | null | undefined)?.closest?.(ENTER_DE_OTROS);
}
