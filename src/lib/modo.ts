// src/lib/modo.ts
//
// La misma app sirve para el host y para los jugadores:
//  * Host: dentro de Tauri (la app de escritorio). Los datos salen de Rust.
//  * Jugador: en un navegador normal, abierta con el enlace de la sala (de GitHub Pages, o servida por la app del
//    host con la dirección propia o en red local). Los
//    datos salen del host, que solo le envía lo que puede ver (ver $lib/backend y docs/red.md).
// En los tests (Node, sin `window`) es el host, así que los mocks de @tauri-apps/api siguen sirviendo.
export const esJugador = typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);
