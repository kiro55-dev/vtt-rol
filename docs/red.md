# Red: la sala del host y los jugadores

Estado: **fase 1**. El host abre una sala; los jugadores entran con un enlace en el navegador y ven **la misma
app que el host** (zonas, mapas, banco, «Fichas activas», tablero…), pero solo con lo que el host comparte, en
tiempo real. Todavía no pueden cambiar nada: eso es la fase 2 (al final está exactamente lo que falta).

## Cómo funciona

```
Navegador del jugador                                  App del host (Tauri)
https://kiro55-dev.github.io/vtt-rol/#t=…&s=…&k=…
  │  la misma app (src/), compilada para la web; el # nunca sale del navegador
  ├─ wss://xxxx.trycloudflare.com/ws ──► cloudflared ──► 127.0.0.1:<puerto>  (src-tauri/src/red/servidor.rs)
  └─ https://xxxx.trycloudflare.com/imagen?…                    │
                                                         EstadoPartida (Rust, la fuente de verdad)
```

- **El host es el único servidor.** Los datos van host → jugador y jugador → host, nunca entre jugadores.
- **Una sola app, dos modos** (`src/lib/modo.ts`): dentro de Tauri es el host; en un navegador normal, un jugador.
  Toda la app habla con sus datos a través de `src/lib/backend.ts` (`invoke` y `listen`), nunca importando
  `@tauri-apps/api` directamente:
  - **Host:** `invoke` es Tauri tal cual (Rust).
  - **Jugador:** `invoke` lo contesta la **réplica** de su vista (`src/lib/red/replica.ts`). El host le envía su
    vista filtrada al entrar y después solo lo que cambia (`Vista` y `Cambios`); la conexión
    (`src/lib/red/conexion.svelte.ts`) la aplica y emite `partida-cambiada`, y los stores releen como hace el
    host tras cada acción suya. Es lo mismo que «pedírselo al host», pero sin una ida y vuelta por cada lectura.
  - Lo que solo puede hacer el host (crear, borrar, renombrar, arrastrar, ⚙, editar fichas, dados…) está oculto
    o desactivado en modo jugador (`esJugador`), y la réplica rechaza cualquier comando que no sea de lectura.
- **Qué comparte el host:** cada mapa tiene una casilla arriba a la derecha (en la lista de mapas). Marcada, los
  jugadores ven ese mapa (y su zona) al instante; desmarcada, deja de verse. Es independiente del mapa que el host
  tenga abierto: puede preparar otros sin que se vean. Cada jugador ve además sus propias fichas (las que el host le
  da arrastrándolo sobre ellas en «Configuración de sala»), enteras.
- **El túnel** es un *quick tunnel* de Cloudflare (`cloudflared`, gratis y sin cuenta). Da una dirección
  `https://<palabras>.trycloudflare.com` distinta cada vez que se abre la sala. `cloudflared` no va en el
  instalador: se descarga la primera vez que se abre una sala (versión fijada y huella SHA-256 comprobada, en
  `src-tauri/src/red/tunel.rs`) a `<datos de la app>/bin/`.
- **La página de los jugadores está en una dirección fija** (GitHub Pages). Eso es lo que permite al navegador
  recordar al jugador aunque el túnel cambie.
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
  `*.trycloudflare.com` (el navegador rechaza cualquier otro `t`). Mensajes de 16 KiB como mucho.
- **Lo que sale hacia cada jugador** pasa siempre por `red::vista::vista_para`, que usa los filtros de
  `visibilidad.rs`: los mapas compartidos y sus zonas, sus fichas (enteras) y las demás instancias de esos mapas sin
  lo privado. Las definiciones de parámetros que salen son solo las de lo que ve (ni los nombres de los privados).
  Las imágenes (`/imagen`) solo se sirven si el jugador de esa sesión las ve (`recurso_visible_para`); la sesión
  de imágenes es una por jugador y se conserva al reconectar. La papelera, los mapas no compartidos y las fichas
  solo del host nunca salen.
- **El chat de tiradas** (fase 2) solo aceptará tiradas: el comando viaja como texto, el host lo interpreta con
  `parsear_comando` y el desglose lo forma Rust con números; además el front lo pinta con `desgloseSeguro`.
- **Adjuntos de jugadores** (fase 2): solo `.txt` (`adjunto_permitido_a_jugador`). PDF prohibidos.

## Publicar la app de los jugadores (una vez)

1. El repositorio tiene que ser **público** (GitHub Pages es gratis en públicos; en uno privado hace falta GitHub
   Pro, o publicar `build-web/` en Cloudflare Pages).
2. En GitHub: *Settings → Pages → Build and deployment → Source: GitHub Actions*.
3. Sube a `main`: la acción `.github/workflows/cliente-web.yml` pasa los tests, compila (`bun run web:build`, que
   genera `build-web/` con la ruta `/vtt-rol`) y publica en `https://kiro55-dev.github.io/vtt-rol/`. Se vuelve a
   publicar sola cuando cambia el front (también se puede lanzar a mano en *Actions*).

Si la dirección cambia (otro usuario u otro nombre de repositorio), cambia `URL_CLIENTE` en
`src-tauri/src/red/mod.rs` y `OPENROLL_BASE` (o el valor por defecto de `paths.base` en `svelte.config.js`).

**Versiones:** la página publicada y la app del host deben hablar la misma versión del protocolo
(`VERSION_PROTOCOLO` en `src-tauri/src/red/protocolo.rs` y `src/lib/red/protocolo.ts`). Si no coinciden, el host
rechaza al jugador con «Versión distinta: recarga la página». Al cambiar un mensaje, sube la versión en los dos
lados y publica la página a la vez que la app.

## Probar en local

```sh
bun run web:dev                                                 # la app de jugador en http://localhost:5173/
OPENROLL_URL_CLIENTE=http://localhost:5173/ bun run tauri dev   # el host genera enlaces hacia ahí
```
En PowerShell: `$env:OPENROLL_URL_CLIENTE="http://localhost:5173/"; bun run tauri dev`.
El túnel sigue siendo real (hace falta internet). En compilaciones de depuración el host acepta además el origen
`http://localhost:5173`. Abre el enlace en otro navegador (o en el móvil, pero entonces la app tiene que estar
publicada: el móvil no ve tu `localhost`).

Tests: `cd src-tauri && cargo test` (sala, vista, protocolo, el saludo completo del servidor por WebSocket y las
imágenes) y `bunx vitest run` (enlace, réplica, respuesta al reto con el mismo vector que Rust, stores).

## Archivos

| | |
|---|---|
| `src-tauri/src/red/mod.rs` | `Red`: la sala abierta, conexiones, `tras_cambio` (enganche de `partida::registrar`), `URL_CLIENTE` |
| `red/sala.rs` | `sala.json`: identidad del enlace, contraseña, claves de los jugadores recordados |
| `red/servidor.rs` | axum en 127.0.0.1: `/ws` (saludo y envíos) y `/imagen` |
| `red/tunel.rs` | descarga verificada de cloudflared y arranque del túnel |
| `red/vista.rs` | qué ve cada jugador y la diferencia con lo último enviado |
| `red/protocolo.rs` | los mensajes (espejo: `src/lib/red/protocolo.ts`) |
| `red/comandos.rs` | comandos de la ventana «Configuración de sala» |
| `src/lib/modo.ts`, `src/lib/backend.ts` | host o jugador; la puerta única de datos |
| `src/lib/red/` | lo del jugador: enlace, conexión, réplica |
| `src/lib/sala.svelte.ts`, `VentanaSala.svelte`, `arrastreJugador.svelte.ts` | el lado del host en la interfaz |
| `EntradaSala.svelte` | nombre y contraseña del jugador al entrar |

## Lo que falta para el cliente completo (fase 2)

**El camino es el mismo para todo:** el `invoke` del jugador, para los comandos que no sean de lectura, envía al
host `Pedir { id, cmd, args }` (mensaje nuevo; subir `VERSION_PROTOCOLO`). El host solo acepta una **lista blanca**
de comandos, comprueba los permisos de `jugadores.rs`, registra el delta como cualquier otro (`partida::registrar`)
y contesta `Respuesta { id, ok | error }`. El cambio llega a todos los jugadores por `tras_cambio`, y al host hay que
emitirle `partida-cambiada` (el store `partida` ya lo escucha y relee). En el front, se quita la restricción de
`esJugador` de cada acción que se habilite. Lo que hay que habilitar (entre paréntesis, lo que ya está en Rust):

1. **Mover su instancia** a una casilla, **varias a la vez**, **girarla**, **espejarla** y **quitarla del
   tablero** (sigue activa). Permiso: `jugadores::controla_instancia`. Reglas: `instancias::mover`, `mover_grupo`,
   `rotar`, `espejar`. Solo en mapas compartidos. En el front: quitar el `esJugador` de `arrastre.preparar` y del
   clic derecho del tablero, y comprobar también allí que la ficha es suya.
2. **Valores actuales de sus fichas** (vida, etc.): `instancias::cambiar_valor_actual` con `controla_instancia`.
   Decidir si el host lo permite o solo lo ve.
3. **Editar su ficha** en la misma ventana del host: el host filtra los `CambioEntidad` con
   `jugadores::cambio_permitido` (no puede cambiar propietarios, zona ni si es única) y valida como
   `modificar_entidad`. Las fotos nuevas necesitarían subir la imagen al host (endpoint nuevo).
4. **Adjuntos .txt de su ficha**: leer, crear y editar. Solo `.txt` (`adjunto_permitido_a_jugador`), tamaño máximo
   `MAX_TEXTO_JUGADOR`, rutas por `archivos::ruta_segura`. Servirlos como texto plano (nunca HTML).
5. **Tiradas**: el host usa `parsear_comando` (solo tiradas), registra `TiradaRealizada` con `Tiradas.jugador` y el
   historial se comparte (añadir `historial_tiradas` a la vista; quitar el `esJugador` del chat y la barra de dados).
6. **Ping y recorrido** del jugador en un mapa compartido: hoy la réplica los ignora; enviarlos al host, validar con
   `AvisoTablero::validar` y reenviar a los demás y al host (evento de Tauri `aviso-tablero`, que `Tablero.svelte` ya
   escucha).
7. **Foco** de cada jugador (`AvisoTablero::Foco`) para que el host lo vea en violeta (`partida.idsFocoAjeno`).

**Más:**
8. Ver quién más está conectado (nombre del host y de los demás jugadores, sin nada más).
9. Lista de jugadores en el host con su foco y sus últimas acciones; posibilidad de bloquear acciones («solo mirar»).
10. Nombre de la sala visible para el jugador.
11. Opcional: **directorio** para que el enlace no cambie entre sesiones (un Worker gratuito que diga «esta sala
    está ahora en este túnel»). Se añade sin tocar nada más: el navegador lo consultaría si el enlace no trae `t`.
12. Opcional: miniatura grande (p. ej. 2048 px) de los fondos de mapa para el jugador, si los mapas son muy pesados
    para la subida del host.

**Probado a mano antes de dar la fase 2 por buena:** dos jugadores a la vez, móvil, cortes de red (el navegador
reintenta durante un minuto) y cerrar la app del host con gente dentro.
