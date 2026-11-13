# Fase 4: relés de Nostr como buzón de la señalización

`bun run experimentos/fase4-nostr/probar.ts` publica un evento efímero (kind 27184, ~3 KB, clave nueva) en cada relé
y comprueba que otra conexión lo recibe por su etiqueta.

Resultado (2026-11-13, desde la red del desarrollador):

| Relé | Resultado |
|---|---|
| `wss://relay.snort.social` | OK, entrega 58 ms |
| `wss://relay.primal.net` | OK, entrega 92 ms |
| `wss://nostr.oxtr.dev` | OK, entrega 155 ms |
| `wss://offchain.pub` | OK, entrega 195 ms |
| `relay.damus.io`, `nos.lol`, `nostr.mom` | no entregan / sin OK |
| `purplepag.es` | «blocked: kind not allowed» |
| `relay.nostr.band`, `nostr.wine`, `relay.nostr.bg`, `nostr-pub.wellorder.net`, `relay.mostr.pub`, `relay.current.fyi` | no conectan |

Lista por defecto (`RELES`): los cuatro que funcionan. Los relés públicos cambian sus reglas: si dejan de valer, se
vuelve a pasar el script y se actualiza la constante en `src-tauri/src/red/senal.rs` y `src/lib/red/senal.ts`.
