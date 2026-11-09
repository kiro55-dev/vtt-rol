# Red: la sala del host y el cliente del jugador

Estado: **fase 1**. El host abre una sala y los jugadores entran con un enlace en el navegador y **ven** en tiempo
real el mapa que el host enseña, con sus fichas. Todavía no pueden hacer nada sobre él: eso es la fase 2 (al
final de este documento está exactamente lo que falta).

## Cómo funciona

```
Navegador del jugador                                  App del host (Tauri)
https://kiro55-dev.github.io/vtt-rol/#t=…&s=…&k=…
  │  página estática (cliente-web/); el # nunca sale del navegador
  ├─ wss://xxxx.trycloudflare.com/ws ──► cloudflared ──► 127.0.0.1:<puerto>  (src-tauri/src/red/servidor.rs)
  └─ https://xxxx.trycloudflare.com/imagen?…                    │
                                                         EstadoPartida (Rust, la fuente de verdad)
```

- **El host es el único servidor.** Los datos van host → jugador y jugador → host, nunca entre jugadores.
- **El túnel** es un *quick tunnel* de Cloudflare (`cloudflared`, gratis y sin cuenta). Da una dirección
  `https://<palabras>.trycloudflare.com` distinta cada vez que se abre la sala. `cloudflared` no va en el
  instalador: se descarga la primera vez que se abre una sala (versión fijada y huella SHA-256 comprobada, en
  `src-tauri/src/red/tunel.rs`) a `<datos de la app>/bin/`.
- **El cliente del jugador** es una página fija publicada en GitHub Pages (carpeta `cliente-web/`), que reutiliza
  el dibujo del tablero de la app (`src/lib/tablero.ts`, `fichasTablero.ts`, `marcasTablero.ts`). Que la página
  esté siempre en la misma dirección es lo que permite al navegador recordar al jugador aunque el túnel cambie.
- **El enlace** es `URL_CLIENTE#t=<túnel>&s=<sala>&k=<secreto>`. Cambia cada vez que se abre la sala (por el
  túnel), así que hay que volver a pasarlo, pero los jugadores que ya entraron **no** repiten la contraseña.
- **Cada partida es un mundo aparte**: su sala (`<partida>/sala.json`) y sus jugadores (`EstadoPartida.jugadores`)
  son de esa partida. Con la sala abierta no se puede cambiar de partida.

## Seguridad

- **El secreto del enlace** (`s` y `k`, 128 bits cada uno) va tras el `#`: el navegador nunca lo envía a GitHub
  ni a nadie. Sin él no se pasa del primer mensaje. *Generar enlace nuevo* cambia `s` y `k`: el enlace viejo deja
  de valer, se echa a los conectados y todos repiten la contraseña (por si se ha filtrado).
- **Contraseña opcional**, guardada como hash Argon2id en `sala.json`. 5 fallos por conexión y 20 por minuto en
  total; medio segundo de espera tras cada fallo.
- **Recordar al jugador sin que el token viaje.** Al entrar por primera vez, el host le da un token aleatorio
  (256 bits) que su navegador guarda en `localStorage` de la página fija. El host solo guarda `SHA-256(token)`.
  Las siguientes veces, el host manda un reto aleatorio y el navegador responde
  `HMAC-SHA256(SHA-256(token), "<reto>|<túnel>")`. Como la respuesta va ligada al reto y a la dirección del
  túnel, un enlace falso que lleve a otro servidor no puede robar nada. Si un navegador que ya había entrado ve
  que la sala no lo reconoce, avisa de que desconfíe antes de poner la contraseña.
- **Solo se acepta el WebSocket desde el origen de la página** (`Origin`), y solo hacia túneles
  `*.trycloudflare.com` (el cliente rechaza cualquier otro `t`). Mensajes de 16 KiB como mucho.
- **Lo que sale hacia cada jugador** pasa siempre por `red::vista::vista_para`, que usa los filtros de
  `visibilidad.rs`: solo el mapa público (`EstadoPartida.mapa_publico`, que el host elige aparte del que tiene
  abierto), sus fichas (enteras) y las demás instancias de ese mapa sin lo privado. Las definiciones de
  parámetros que salen son solo las de lo que ve (ni los nombres de los privados). Las imágenes (`/imagen`) solo
  se sirven si el jugador de esa sesión las ve (`recurso_visible_para`); la papelera, los mapas ocultos y las
  fichas solo del host nunca salen.
- **El chat de tiradas** (fase 2) solo acepta tiradas: el comando viaja como texto, el host lo interpreta con
  `parsear_comando` y el desglose lo forma Rust con números; además el front lo pinta con `desgloseSeguro`.
- **Adjuntos de jugadores** (fase 2): solo `.txt` (`adjunto_permitido_a_jugador`). PDF prohibidos.

## Publicar el cliente (una vez)

1. El repositorio tiene que ser **público** (GitHub Pages es gratis en públicos; en uno privado hace falta GitHub
   Pro, o publicar `cliente-web/dist` en Cloudflare Pages).
2. En GitHub: *Settings → Pages → Build and deployment → Source: GitHub Actions*.
3. Sube a `main`: la acción `.github/workflows/cliente-web.yml` comprueba, construye y publica en
   `https://kiro55-dev.github.io/vtt-rol/`. Se vuelve a publicar sola cuando cambian el cliente o el dibujo del
   tablero que comparte con la app (también se puede lanzar a mano en *Actions*).

Si la dirección cambia (otro usuario u otro nombre de repositorio), cambia `URL_CLIENTE` en
`src-tauri/src/red/mod.rs` y la `base` de `cliente-web/vite.config.ts`.

**Versiones:** la página publicada y la app del host deben hablar la misma versión del protocolo
(`VERSION_PROTOCOLO` en `src-tauri/src/red/protocolo.rs` y `cliente-web/src/protocolo.ts`). Si no coinciden,
el host rechaza al jugador con «Versión distinta: recarga la página». Al cambiar un mensaje, sube la versión en
los dos lados y publica la página a la vez que la app.

## Probar en local

```sh
bun run web:dev                                          # cliente en http://localhost:5173/
OPENROLL_URL_CLIENTE=http://localhost:5173/ bun run tauri dev   # el host genera enlaces hacia ahí
```
En PowerShell: `$env:OPENROLL_URL_CLIENTE="http://localhost:5173/"; bun run tauri dev`.
El túnel sigue siendo real (hace falta internet). En compilaciones de depuración el host acepta además el origen
`http://localhost:5173`. Abre el enlace en otro navegador (o en el móvil, pero entonces el cliente tiene que
estar publicado: el móvil no ve tu `localhost`).

Tests: `cd src-tauri && cargo test` (sala, vista, protocolo y el saludo completo del servidor por WebSocket) y
`bunx vitest run` (enlace, cambios, respuesta al reto con el mismo vector que Rust, store de la sala).

## Archivos

| | |
|---|---|
| `src-tauri/src/red/mod.rs` | `Red`: la sala abierta, conexiones, `tras_cambio` (enganche de `partida::registrar`), `URL_CLIENTE` |
| `red/sala.rs` | `sala.json`: identidad del enlace, contraseña, claves de los jugadores recordados |
| `red/servidor.rs` | axum en 127.0.0.1: `/ws` (saludo y envíos) y `/imagen` |
| `red/tunel.rs` | descarga verificada de cloudflared y arranque del túnel |
| `red/vista.rs` | qué ve cada jugador y la diferencia con lo último enviado |
| `red/protocolo.rs` | los mensajes (espejo: `cliente-web/src/protocolo.ts`) |
| `red/comandos.rs` | comandos de la ventana «Configuración de sala» |
| `src/lib/sala.svelte.ts`, `VentanaSala.svelte`, `arrastreJugador.svelte.ts` | el lado del host en la interfaz |
| `cliente-web/` | la página del jugador |

## Lo que falta para el cliente completo (fase 2)

Lo que ya está preparado en Rust aparece entre paréntesis.

**Mensajes jugador → host** (hoy el servidor los ignora: `// FUTURA RED (fase 2)` en `servidor.rs`). Cada uno se
valida en el host y se convierte en un delta normal (`partida::registrar`), con lo que llega a todos con
`tras_cambio`:
1. **Mover su instancia** a una casilla, **varias a la vez**, **girarla**, **espejarla** y **quitarla del
   tablero** (sigue activa). Permiso: `jugadores::controla_instancia`. Reglas: `instancias::mover`,
   `mover_grupo`, `rotar`, `espejar`. Solo en el mapa público.
2. **Valores actuales de sus fichas** (vida, etc.): `instancias::cambiar_valor_actual` con
   `controla_instancia`. Decidir si el host lo permite o solo lo ve.
3. **Editar su ficha**: el cliente manda `CambioEntidad`s; el host filtra con `jugadores::cambio_permitido`
   (no puede cambiar propietarios, zona ni si es única) y valida como `modificar_entidad`.
4. **Adjuntos .txt de su ficha**: leer, crear y editar. Solo `.txt` (`adjunto_permitido_a_jugador`), tamaño máximo
   `MAX_TEXTO_JUGADOR`, rutas por `archivos::ruta_segura`. Servirlos como texto plano (nunca HTML).
5. **Tiradas**: el jugador manda el comando como texto; el host usa `parsear_comando` (solo tiradas), registra
   `TiradaRealizada` con `Tiradas.jugador` y el historial se comparte (añadir `historial_tiradas` a la vista).
6. **Ping y recorrido** del jugador en el mapa público: validar con `AvisoTablero::validar` y reenviar a los
   demás y al host (evento de Tauri `aviso-tablero`, que `Tablero.svelte` ya escucha).
7. **Foco** de cada jugador (`AvisoTablero::Foco`) para que el host lo vea en violeta (`partida.idsFocoAjeno`).

**Cliente web:**
8. Arrastrar sus fichas en el tablero, menú de giro, caja de parámetros, editor de su ficha y de sus .txt,
   chat de tiradas y barra de dados (se pueden reutilizar `aplicarDadoAlComando` y `desgloseSeguro`).
9. Ver quién más está conectado (nombre del host y de los demás jugadores, sin nada más).

**Host:**
10. Lista de jugadores con su foco y sus últimas acciones; posibilidad de bloquear acciones (p. ej. «solo mirar»).
11. Nombre de la sala visible en la página del jugador.
12. Opcional: **directorio** para que el enlace no cambie entre sesiones (un Worker gratuito que diga «esta sala
    está ahora en este túnel»). Se añade sin tocar nada más: el cliente lo consultaría si el enlace no trae `t`.

**Probado a mano antes de dar la fase 2 por buena:** dos jugadores a la vez, móvil, cortes de red (el cliente
reintenta durante un minuto) y cerrar la app del host con gente dentro.
