# Red: la sala del host y los jugadores

Estado: **fase 2**. El host abre una sala; los jugadores entran con un enlace en el navegador y usan **la misma app
que el host** (zonas, mapas, banco, «Fichas activas», tablero, dados y chat), pero solo con lo que el host
comparte, y solo pueden actuar sobre las fichas que el host les da. Cada acción del jugador se la pide al host,
que la valida y la ejecuta en su Rust; el resultado llega a todos en tiempo real.

## Cómo funciona

```
Navegador del jugador                                  App del host (Tauri)
https://kiro55-dev.github.io/OpenRoll/#t=…&s=…&k=…
  │  la misma app (src/), compilada para la web; el # nunca sale del navegador
  ├─ wss://xxxx.trycloudflare.com/ws ──► cloudflared ──► 127.0.0.1:<puerto>  (src-tauri/src/red/servidor.rs)
  │    (juego e imágenes por la misma conexión)                 │
  └─ https://xxxx.trycloudflare.com/imagen?…  (solo respaldo, sin Service Worker)
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
  - **Acciones del jugador:** lo que cambia la partida (mover, tirar dados, editar su ficha…) se lo pide al host
    (`Pedir { id, peticion }` → `Respuesta { id, ok | error }`, ver «Acciones del jugador»). Lo que solo es del
    host (⚙, crear, borrar, renombrar o reordenar, el orden de «Fichas activas», limpiar el chat) está oculto o
    desactivado en modo jugador (`esJugador`) y el host lo rechaza de todos modos.
- **Qué comparte el host:** cada mapa tiene una casilla arriba a la derecha (en la lista de mapas). Marcada, los
  jugadores ven ese mapa (y su zona) al instante; desmarcada, deja de verse. Es independiente del mapa que el host
  tenga abierto: puede preparar otros sin que se vean. Cada jugador ve además sus propias fichas (las que el host le
  da arrastrándolo sobre ellas en «Configuración de sala»), enteras.
- **Cómo se publica** (en «Configuración de sala» → «Cómo llegan los jugadores»; es de este equipo, no de la
  partida: `<datos de la app>/red.json`, `src-tauri/src/red/config.rs`; solo se cambia con la sala cerrada):
  - **Túnel de Cloudflare** (por defecto): un *quick tunnel* (`cloudflared`, gratis y sin cuenta). Da una dirección
    `https://<palabras>.trycloudflare.com` distinta cada vez que se abre la sala. `cloudflared` no va en el
    instalador: se descarga la primera vez que se abre una sala (versión fijada y huella SHA-256 comprobada, en
    `src-tauri/src/red/tunel.rs`) a `<datos de la app>/bin/`.
  - **Dirección propia**: el host pone su URL https (su dominio con su propio túnel de Cloudflare, `tailscale
    funnel`, `ngrok`, un proxy https con un puerto abierto…) y un puerto; la app escucha en `127.0.0.1:<puerto>` y
    el enlace es esa dirección (`https://mi.dominio.es/#s=…&k=…`), que no cambia al reabrir. Lo que lleve de la URL
    a ese puerto lo monta el host. Solo https y sin rutas (`config::direccion_de_url`). Si el puerto está ocupado, la
    sala no abre y lo dice.
  - **Red local / VPN** («abrir en LAN»): la app escucha en **todas** las interfaces (`0.0.0.0:<puerto>`) y la
    ventana da un enlace por cada IPv4 del equipo con el nombre de su red (`config::direcciones_lan`, con
    `if-addrs`; p. ej. «Radmin VPN — `http://26.x.x.x:8080/`»). **El enlace va
    sin llave** (sin `s` ni `k`), como en los juegos: la confianza está en la red, y `SalaAbierta::sin_llave` deja
    pasar el saludo sin ella solo en este modo. Quien llegue a esa IP y puerto puede entrar (con el nombre, y la
    contraseña si el host la puso: sigue siendo opcional, y los topes de jugadores y de conexiones siguen valiendo).
    No hay «generar enlace nuevo» (no hay llave que cambiar). **Sin cifrar**: la contraseña y el token viajan en
    claro dentro de esa red (Radmin, ZeroTier, Hamachi cifran entre sus equipos; una LAN normal no). Windows pide
    permiso al cortafuegos la primera vez.
- **Conexión directa (peer to peer, WebRTC)**: la forma principal «Peer to peer» (con respaldo Cloudflare o tu
  dirección propia). El jugador habla **directo** con el host, sin pasar por ningún servidor; el respaldo es el
  WebSocket de siempre, para quien tenga una red que no lo permita (NAT estricto) o no quiera. Librería del host:
  `webrtc` 0.21.1 (por qué esta: ver «Por qué webrtc-rs», abajo). Los dos se ponen de acuerdo **solos**, por relés de Nostr.
  - **Cómo se conecta** (el jugador no toca nada): si el enlace lleva la huella (`&f=…`) y el navegador puede (WebRTC,
    WebCrypto y una página https), `conexion.svelte.ts` intenta lo directo: abre los canales, publica su oferta en los
    relés (`src/lib/red/senal.ts`) y espera la respuesta del host (hasta 10 s); si llega, comprueba la huella y espera a
    que el host hable (hasta 12 s). **Cualquier fallo o tiempo agotado → entra por el respaldo sin pedir nada** (relés
    caídos, NAT estricto, navegador sin WebRTC, reloj mal puesto…). Si la directa se cae en pleno juego, se sigue por el
    respaldo.
  - **Señalización por relés de Nostr** (`red/senal.rs` ↔ `senal.ts`): el contenido es el MISMO código cifrado de más
    abajo (no hay segundo cifrado) dentro de un evento efímero (kind 27184, que los relés reenvían y no guardan),
    firmado con una clave Schnorr nueva por evento. Las etiquetas con las que se encuentran salen del secreto del
    enlace (HMAC-SHA256, «oferta» y «respuesta»; la respuesta lleva además la etiqueta `x` con el `n` de la oferta). El
    host (`senal::arrancar`, al abrir la sala en peer to peer) escucha en `senal::RELES` (una tarea por relé, reconecta
    con espera de 1 s a 60 s) y contesta con `webrtc::responder_con`. Topes: 6 ofertas por minuto, eventos ≤ 32 KiB,
    `created_at` a ±2 min, y el mismo evento (llega por varios relés) una vez, además de los de abajo. La lista de relés
    es una constante en Rust y en TS (a cambiar a la vez, como el protocolo); se eligió con
    pruebas con relés reales (ver «Relés de Nostr», abajo): los relés públicos cambian sus reglas. Con «Generar enlace
    nuevo» la escucha se rehace (las etiquetas cambian con el secreto). El TLS de los relés usa un proveedor de rustls
    explícito (`ring`): en el proceso conviven `aws-lc-rs` (reqwest) y `ring` (webrtc), y el «por defecto» entraría en
    pánico. Rust y TS comparten un vector fijo (id y firma del evento, etiquetas).
  - **Qué ven los relés**: la IP de quien publica (host y cada jugador), la hora, el tamaño y una etiqueta opaca. NO
    ven el SDP ni las IPs de los candidatos (cifrado) ni a qué sala va (etiqueta derivada del secreto). Aunque lo
    alteraran, el navegador rechaza una respuesta con otra huella.
  - **Para el host** (desarrollo): en «Configuración de sala», cada jugador conectado lleva una etiqueta **«Directo»**
    o **«Respaldo»** (**«Red local»** en ese modo; viene de `Transporte.via` → `Conexion.via` → `JugadorSala.via`), y
    la sala abierta en peer to peer enseña «relés conectados N de M» (`EstadoSala.senal`).
  - **«Túnel»** (tercera opción principal junto a «Peer to peer» y «Red local»; `ConfigRed.solo_respaldo` en `red.json`, `SalaAbierta::directa()`):
    no se ofrece lo directo: el enlace sale sin `&f=`, no se crea el certificado ni se escucha en los relés, y todos
    entran por el respaldo; los jugadores nunca ven la IP del host (con Cloudflare, el túnel la esconde). Un enlace viejo
    con `f` espera ~10 s y cae solo al respaldo (regenerar el enlace lo evita). Los enlaces de Cloudflare también llevan
    la huella cuando se ofrece lo directo.
  - **Volver a lo directo**: si una directa que había funcionado se cae, el jugador sigue por el respaldo y a los 30 s,
    2 min y 5 min prueba la directa **en segundo plano** (`#mejorarADirecto`, oyentes diferidos): si habla, se cambia con
    el camino de una reconexión (se cierra el WebSocket, la directa saluda y llega la vista); si no, no se entera. Solo si
    hay token guardado (si no, pediría el nombre otra vez). Los mensajes de un transporte viejo se ignoran (si no, el
    `Rechazado` «has entrado desde otra pestaña» del host cortaría la sesión).
  - **Recordar el fallo**: si lo directo falla al entrar, se guarda `openroll.directo.<sala>` en el navegador y en
    los próximos 30 min se entra ya por el respaldo, sin esperar; si funciona, se borra.
  - **Tiempos**: ICE fallido a los 30 s y plaza a medias liberada a los 45 s (antes 300 s y 10 min, por los códigos a
    mano), para que los intentos que fallan no llenen las 8 plazas del host.
  - **Códigos** (`red/codigo.rs` ↔ `src/lib/red/codigo.ts`): `base64url(nonce ‖ AES-256-GCM(deflate(JSON {tipo, sdp, t,
    n})))` con clave HKDF-SHA256 del secreto del enlace. Un SDP lleva IPs: sin el enlace no se lee ni se fabrica uno
    que el host acepte. El host rechaza los de más de 10 minutos, los repetidos y más de 8 a medias
    (`webrtc::MAX_PEERS_A_MEDIAS`). Los dos lados descifran un código fijo hecho por el otro (tests).
  - **Identidad del host**: `sala.json` guarda un certificado DTLS (`certificado_dtls`, solo del host; se renueva con
    «Generar enlace nuevo»). Su huella SHA-256 va en el enlace (`f`, base64url de 43 caracteres) y el navegador **no
    acepta una respuesta con otra** (`usarRespuesta`): quien viera el código no puede hacerse pasar por el host. La
    prueba del token por esta vía va atada a `webrtc:<huella>` (`SalaAbierta::direcciones`).
  - **Tres canales** (los abre el navegador): `juego` (fiable y ordenado: el protocolo en trozos de ≤ 16 KiB,
    `red/trozos.rs` ↔ `trozos.ts`, tope de 512 KiB por mensaje), `imagenes` (fiable y ordenado: los trozos de imagen,
    aparte para no retrasar al juego) y `arrastre` (**sin orden ni reintentos**: el arrastre en vivo; si se pierde uno,
    el siguiente lo sustituye). Con la conexión directa se arma un `Transporte` y corre el **mismo `atender`**: saludo,
    contraseña, permisos y topes son los mismos que por el túnel.
  - **Protocolo 7**: `MensajeCliente::Aviso { aviso }` (el arrastre del jugador, sin respuesta ni cola) y
    `AvisoTablero::Arrastre`/`ArrastreFin` con un número `n` creciente por pantalla; `arrastreAjeno` ignora uno más
    viejo que el último visto (y un `Arrastre` retrasado justo tras soltar). Por el WebSocket todo va por el único canal.
  - El host contesta en cuanto tiene el primer candidato público (`srflx`) + 300 ms, o a los 1,5 s, sin esperar a que
    la recogida «termine» (en Linux a veces no llegaba a tiempo: fase 0). Tiempos ICE de 5 minutos (los 25 s de serie
    cerraban antes de que llegara la respuesta). El firewall de Windows pedirá permiso para la red (UDP).
  - En **red local** no hay conexión directa de este tipo: ya llegan directos por `http` (sin certificado, sin huella).
  - **Privacidad**: en directo, host y jugador ven la IP pública del otro (los navegadores esconden las locales).
    Quien no quiera, no usa «Peer to peer» (red local, o dirección propia sin huella).
- **La página de los jugadores va dentro de la app** (`build-jugador/`, `bun run web:build:app`, raíz `/`; la
  incluyen `beforeDevCommand`/`beforeBuildCommand`). El servidor de la sala la sirve en `/` (`red/pagina.rs`, con
  `rust-embed`: en desarrollo la lee del disco, en release va en el ejecutable). Con la dirección propia o en red
  local el enlace NO lleva `t`: el navegador entiende que el host es quien le sirvió la página (mismo origen), con
  su esquema (`ws://` en red local). Página y servidor salen de la misma app: nunca hay «versión distinta». Con el
  túnel de Cloudflare se sigue usando la de GitHub Pages, porque su dirección cambia cada vez y el navegador
  perdería la memoria de los jugadores. Cabeceras: `index.html` y el Service Worker con `no-cache`, lo de
  `_app/immutable/` con caché para siempre, y `nosniff`, `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`.
- **En `http://` (red local) el navegador no es «contexto seguro»**: no hay `crypto.subtle` (la prueba del token
  usa `@noble/hashes` en todos los modos) ni Service Worker (las imágenes van por el respaldo `/imagen`, relativo).
  - (Fase 3 del plan WebRTC: conexión directa, con una de estas como respaldo.)
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
- **Solo se acepta el WebSocket desde el origen de la página** (`Origin`, `Red::acepta_origen`): el de GitHub Pages o
  los de la propia sala abierta (`https://<dirección propia>`, `http://<ip>:<puerto>` de cada red), nunca lo que diga
  la cabecera `Host`. La prueba del token se comprueba contra las direcciones de la sala (`SalaAbierta::direcciones`):
  una hecha para otro servidor no vale. El navegador solo acepta como `t` una
  dirección con forma de dominio (y puerto opcional), sin rutas ni usuarios, y siempre por `wss`/`https`. Ya no se
  limita a `*.trycloudflare.com` (por la dirección propia); un enlace falso sigue sin poder robar el token, porque
  la prueba va atada a la dirección. Mensajes de 512 KiB como mucho.
- **Lo que sale hacia cada jugador** pasa siempre por `red::vista::vista_para`, que usa los filtros de
  `visibilidad.rs`: los mapas compartidos y sus zonas, sus fichas (enteras) y las demás instancias de esos mapas sin
  lo privado. Las definiciones de parámetros que salen son solo las de lo que ve (ni los nombres de los privados).
  Las imágenes (`/imagen`) solo se sirven si el jugador de esa sesión las ve (`recurso_visible_para`); la sesión
  de imágenes es una por jugador y se conserva al reconectar. La papelera, los mapas no compartidos y las fichas
  solo del host nunca salen.
- **El chat de tiradas** (fase 2) solo aceptará tiradas: el comando viaja como texto, el host lo interpreta con
  `parsear_comando` y el desglose lo forma Rust con números; además el front lo pinta con `desgloseSeguro`.
- **Adjuntos de jugadores** (fase 2): solo `.txt` (`adjunto_permitido_a_jugador`). PDF prohibidos.
- **Topes contra el abuso** (quien tenga el enlace no puede romper la partida ni llenar el disco del host):
  - Altas: máximo `MAX_JUGADORES` (32) en la partida; como mucho `MAX_SIN_IDENTIFICAR` (16) conexiones a la
    vez sin ser nadie, con 180 s para escribir el nombre.
  - Nombres: sin caracteres invisibles ni de control o de dirección (U+202E…), sin repetir el de otro jugador
    (da igual mayúsculas y espacios) y sin los reservados («rollmaster», «host», «máster»…), para que nadie se haga
    pasar por otro en el chat (`jugadores::registrar`). Los nombres de ficha tampoco admiten esos caracteres.
  - Instancias: sin ids repetidos en una petición y máximo `MAX_INSTANCIAS_PROPIAS` (30) instancias de sus
    fichas a la vez. Si los ids de instancia llegan a 65535 se reutiliza el menor libre (`instancias.rs`).
  - Archivos: máximo `MAX_ADJUNTOS_JUGADOR` (10) archivos y 2 MiB por carpeta de ficha con lo que crea un jugador;
    en cualquier ficha, nombre ≤ 100 caracteres, texto de parámetro ≤ 2000 y ≤ 50 adjuntos sin repetir
    (`validar_entidad`).
  - Imágenes subidas: ≤ 8 MiB, ≤ 8192 px por lado y ≤ 40 Mpx (una «bomba» de pocos MB colgaría a quien la abra);
    la miniatura propia de una ficha, WebP de ≤ 512 px. Las subidas que ninguna ficha usa se borran al cerrar la
    sala, al expulsar al jugador y al generar enlace nuevo (`recursos::descartar`, nunca borra lo que se usa).
  - Colas: máximo 64 peticiones esperando por jugador; si un jugador acumula 2000 mensajes sin recibir (o tarda
    más de 30 s en uno) se le corta y al volver recibe su vista entera.
- **Webview del host**: CSP en `tauri.conf.json` (solo scripts propios, imágenes de `asset:`/`blob:`/`data:`) y el
  protocolo `asset:` limitado a la carpeta de datos (`$APPDATA/**`), así que ni un fallo futuro al pintar texto de
  un jugador daría acceso al resto del disco. En `tauri dev` no hay CSP (`devCsp: null`): **prueba la build de
  release** tras tocar cómo se cargan scripts, estilos o imágenes.
- **Privacidad**: las imágenes se sirven tal cual. Una foto de móvil puede llevar la ubicación GPS en sus metadatos
  EXIF: si es tuya, quítaselos antes de usarla como fondo o foto de una ficha que vean los jugadores.
- **El origen de la página** (`https://<usuario>.github.io`) lo comparten todas las páginas de GitHub de esa
  cuenta, también su `localStorage` (donde vive el token). No publiques en esa cuenta otras páginas con código de
  terceros, o pon la app de los jugadores en un dominio propio.

## Publicar la app de los jugadores (una vez)

1. El repositorio tiene que ser **público** (GitHub Pages es gratis en públicos; en uno privado hace falta GitHub
   Pro, o publicar `build-web/` en Cloudflare Pages).
2. En GitHub: *Settings → Pages → Build and deployment → Source: GitHub Actions*.
3. Sube a `main`: la acción `.github/workflows/cliente-web.yml` pasa los tests, compila (`bun run web:build`, que
   genera `build-web/` con la ruta `/OpenRoll`) y publica en `https://kiro55-dev.github.io/OpenRoll/`. Se vuelve a
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
| `red/servidor.rs` | axum en 127.0.0.1: `/ws` y el respaldo `/imagen`; `atender`, que atiende a un jugador de principio a fin |
| `red/transporte.rs` | la conexión con un jugador como dos colas de `Trama` (texto/binario), con contrapresión; adaptador del WebSocket |
| `red/imagenes.rs` | imágenes de los jugadores: qué puede ver cada uno, bajadas y subidas en trozos de 16 KiB |
| `red/tunel.rs` | descarga verificada de cloudflared y arranque del túnel |
| `red/vista.rs` | qué ve cada jugador y la diferencia con lo último enviado |
| `red/protocolo.rs` | los mensajes (espejo: `src/lib/red/protocolo.ts`); versión 6 |
| `red/peticiones.rs` | lo que un jugador puede pedir: lista blanca, permisos y ejecución |
| `red/comandos.rs` | comandos de la ventana «Configuración de sala» |
| `src/lib/modo.ts`, `src/lib/backend.ts` | host o jugador; la puerta única de datos |
| `src/lib/red/` | lo del jugador: enlace, conexión, réplica |
| `src/lib/sala.svelte.ts`, `VentanaSala.svelte`, `arrastreJugador.svelte.ts` | el lado del host en la interfaz |
| `EntradaSala.svelte` | nombre y contraseña del jugador al entrar |

## Acciones del jugador (fase 2)

Cada acción viaja como `Pedir { id, peticion: { cmd, ...args } }` y el host contesta `Respuesta { id, ok | error }`
**después** de los `Cambios` que provocó (van por el mismo canal), así la réplica del jugador ya está al día cuando
su pantalla relee. Las peticiones de un jugador se ejecutan **en orden**, una a una, con un tope de 30 por segundo.

- **Lista blanca** (`red/peticiones.rs`, `Peticion`; espejo en `PETICIONES` de `src/lib/red/replica.ts`): tirar
  dados, mover una o varias instancias, girarlas, espejarlas, sacarlas del tablero (eliminarlas), cambiar su valor
  (vida…), poner fichas del banco en el tablero, guardar su ficha y su miniatura, leer, crear, editar y renombrar
  sus `.txt`, descartar archivos de una edición cancelada, y ping y recorrido. Lo demás ni se decodifica.
- **Permisos** (`permitida`, pura y con tests, sobre `jugadores.rs`): solo sus fichas (`Entidad.propietarios`) e
  instancias; solo llevar fichas a mapas **compartidos** (el host no tiene este límite: puede llevar cualquier
  ficha a cualquier mapa); de su ficha no puede cambiar de quién es, su zona ni si es única (`cambio_permitido`);
  la foto solo si la ha subido él; los adjuntos nuevos, solo `.txt` en `archivos/entidad_<id>/`.
- **Se ejecuta con la misma función que el comando del host.** Cada comando de la lista blanca tiene su
  `xxx_en(…, &AppState)` en `partida.rs` (el comando de Tauri solo la llama): así hay una sola regla y se prueba
  sin Tauri. Al añadir una acción nueva: variante en `Peticion`, caso en `permitida`, caso en `ejecutar_en`,
  nombre en `PETICIONES` y tests.
- **Chat compartido.** `tirar_dado_de(estado, jugador, comando)` lo procesa el Rust del host y lo registra en el
  chat con el nombre de quien tira (`Tiradas.autor`, vacío si es el host). El chat entero va en la vista de cada
  jugador (`historial_tiradas`) y en los `Cambios`: todos ven las tiradas de todos, como «Ana: 1d20 (7) = 7».
  `clean`/`c` solo lo puede usar el host, y solo el host puede tirar en privado (casilla «Pública» de la barra de dados: desmarcada, la tirada no llega a los jugadores). El historial personal (↑ ↓) de cada jugador se guarda en el disco del host (`historial_jugador_<id>.txt`, máx. 100) y lo recupera al reconectar (`obtener_historial_personal`); se borra al quitarlo de la partida.
- **Pantalla del host.** Tras una acción de un jugador el host emite `partida-cambiada` (`tiradas` o `entidades`)
  y su store relee (`partida.alCambiarPartida`); el ping y el recorrido de un jugador llegan al host como el
  evento `aviso-tablero` y a los demás jugadores como `Aviso`.
- **El jugador sigue el mapa del host.** No elige mapa: ve el que el host tiene abierto si lo comparte (el host se lo
  dice a Rust con `abrir_mapa`; efímero, no se guarda). Si el host entra en un mapa que no comparte, el jugador no
  nota nada (sigue en el último compartido), tampoco al volver; si abre otro compartido, lo ve. Las miniaturas de todos
  los mapas compartidos siguen en su lista (pulsarlas no cambia lo que ve). Protocolo 5 (`mapa_visible`).
- **Quitar jugadores.** «Quitar de la partida» (⚙ → Configuración de sala) borra su registro y lo echa de la sala; sus
  fichas no se bloquean ni se borran: pasan a ser solo del host.
- **Arrastre en vivo.** Mientras alguien arrastra una ficha por el tablero, los demás la ven pasar de casilla en casilla
  (semitransparente): `Arrastre` y `ArrastreFin` son avisos efímeros, como el recorrido (protocolo 4). La posición
  guardada solo cambia al soltar. Un jugador solo puede avisar del arrastre de sus fichas. Solo se ve el arrastre de
  fichas que ya estaban puestas en el tablero.
- **Ficha caída.** Si el parámetro que pone una ficha en gris (la vida) es privado, los demás reciben solo la señal
  `atenuada`, no su valor.
- **Foco.** El foco es de cada uno: un jugador puede dar el foco a cualquier ficha que ve (para ver sus formas
  públicas), pero no se envía a nadie: el host no ve el foco de los jugadores ni ellos el suyo.
- **Imágenes (protocolo 6).** Van por la **misma conexión** que el juego, no por HTTP (así funcionarán igual cuando la
  conexión sea WebRTC):
  - El jugador pide `PedirImagen { id, ruta, mini? }`; el host contesta `ImagenCabecera { id, mime, tam }` y la manda en
    trozos binarios de 16 KiB (1 byte de tipo + id en 4 bytes + datos), leyéndola del disco trozo a trozo, o
    `ImagenError` si no la ve (`imagenes::localizar`, la misma regla de siempre: `recurso_visible_para` o subida por
    él). Como mucho 4 a la vez por jugador y 64 en cola; más → `ImagenError("ocupado")` y el navegador reintenta.
  - La salida de cada conexión está acotada (`transporte.rs`): una imagen grande va al ritmo del jugador, se intercala
    con los mensajes del juego y no acumula memoria en el host. A un jugador que no lee en 30 s se le corta.
  - En el navegador, `src/lib/red/imagenesRed.ts` pide (6 a la vez), junta los trozos y guarda lo bajado en la Cache
    API (el nombre es el hash del contenido: nunca se queda viejo; se vacía al salir de la sala). Los `<img>` siguen
    usando una URL normal, `<base>/__imagen?ruta=…`, que contesta el **Service Worker** (`src/service-worker.ts`)
    pidiéndosela a la página. El SW solo hace eso: no guarda la app en caché. Lo registra a mano la conexión del
    jugador (`kit.serviceWorker.register: false`; en la app del host nunca se registra).
  - **Respaldo:** sin Service Worker (p. ej. Firefox en ventana privada), las imágenes se piden por
    `GET /imagen?ruta=…&sesion=…` del túnel, con la misma regla, CORS para el origen de la página y `Vary: Origin`.
- **Fotos.** Un jugador sube la foto de una ficha suya por la conexión: `SubirImagen { id, ext, tam }` y trozos
  binarios → `SubidaOk { id, ruta }` o `SubidaError`. Hasta 8 MiB (también sin tener aún ninguna ficha: así puede
  arrastrar una imagen al banco y crear la suya), solo imágenes de verdad (png, jpg, webp, gif o bmp; **nada de
  SVG**; el contenido debe ser del tipo que dice la extensión), 2 a la vez, y hasta 20 por sesión
  (`imagenes::guardar_subida`). Solo él la ve hasta que la usa una ficha. Ya no hay `POST /subir`.
- **Fichas propias.** Un jugador crea su ficha como el host (borrador con id reservado → guardar / cancelar), pasando por
  el host: `iniciar_borrador_entidad`, `crear_entidad` y `cancelar_borrador_entidad` están en la lista blanca. El host
  elige la zona (la que pide si la ve, o la primera con un mapa compartido), y al guardar **fuerza** `propietarios =
  [jugador]` y `es_unico = false`. Topes: 3 borradores abiertos y 20 fichas por jugador. Los `.txt` de un borrador valen
  igual que los de una ficha suya. Si el jugador se va (sala cerrada, expulsado, enlace nuevo) sus borradores se
  anulan solos (`Red::poner_limpiador`), para no dejar ids reservados. El host la controla después como cualquier ficha.
- **Fondos antiguos.** Versiones muy antiguas guardaban el fondo de un mapa (o la foto de una ficha) como una URL de un
  archivo del disco del host (`http://asset.localhost/…`). Al abrir una partida, `migrar_fondos_antiguos` los copia a
  `imagenes/` (un delta normal por cada uno) para poder medirlos, protegerlos y servirlos a los jugadores; si el archivo
  ya no existe se dejan como estaban.
- **Qué controla un jugador en la pantalla:** `partida.controla(instancia)` (el host siempre; un jugador, las de las
  fichas de su banco). Oculta o desactiva lo que el host va a rechazar, pero la comprobación de verdad es la del host.

## Por qué webrtc-rs

Antes de migrar se compararon dos librerías de WebRTC en Rust, con una prueba aislada (host en Windows y Linux,
jugador en Chrome y Firefox, mismo PC y otra red real). La elegida es **`webrtc` 0.21.1 (webrtc-rs)**:

- Fue la única que conectó **en directo entre dos redes reales** (`srflx → srflx`, sin túnel ni relé): 410 ms para
  conectar, 37 ms de ida y vuelta, 200 de 200 mensajes del arrastre sin pérdidas y 60 Mbit/s de bajada. `str0m`
  funcionó igual en el mismo PC, pero desde otra red no llegó a conectar.
- Reúne ella sola los candidatos ICE (interfaces y STUN), lleva su bucle de red y su certificado DTLS no caduca.
  `str0m` obliga a escribir ese código de red a mano (varias IP, IPv6…), que es justo lo que no se quiere mantener.
- Compila en Windows y Linux (en Linux, `ring` necesita compilador de C), pesa unos +4 MB en el ejecutable y todas sus
  dependencias son de licencias permisivas, compatibles con GPL-3.0.
- Anuncia `max-message-size` de 64 KiB y el navegador no envía más de eso a la vez: por eso los mensajes van en trozos de
  16 KiB (`red/trozos.rs`).
- Hallazgo: en Linux el primer intento tardaba unos 3 s porque el host esperaba a que la recogida de candidatos
  «terminara». Ahora contesta en cuanto hay el primer candidato público (`srflx`) más 300 ms, o a los 1,5 s.

## Relés de Nostr

La lista `senal::RELES` (y su copia en `src/lib/red/senal.ts`) sale de probar relés públicos reales: publicar un evento
efímero (kind 27184) de ~3 KB con una clave nueva y comprobar que otra conexión lo recibe por su etiqueta. Probados el
2026-11-13, desde la red del desarrollador:

- **Funcionaron:** `relay.snort.social` (58 ms de entrega), `relay.primal.net` (92 ms), `nostr.oxtr.dev` (155 ms) y
  `offchain.pub` (195 ms).
- **No sirvieron:** `relay.damus.io`, `nos.lol` y `nostr.mom` no entregaron el evento; `purplepag.es` lo rechaza
  («kind not allowed»); `relay.nostr.band`, `nostr.wine`, `relay.nostr.bg`, `nostr-pub.wellorder.net`,
  `relay.mostr.pub` y `relay.current.fyi` no conectaron.

Los relés públicos cambian sus reglas: si la conexión directa deja de ir, lo primero que hay que revisar es esta lista.
Hay un test que no corre por defecto y comprueba que la escucha del host llega a conectar con los relés reales
(`cargo test reles_reales -- --ignored`).

## Lo que falta

- Ver quién más está conectado (nombre de los demás jugadores).
- Lista de jugadores en el host con sus últimas acciones; bloquear acciones («solo mirar»).
- Nombre de la sala visible para el jugador.
- Subir adjuntos distintos de `.txt` (hoy no: PDF prohibidos) y fotos de mapas.
- Opcional: **directorio** para que el enlace no cambie entre sesiones (un Worker gratuito que diga «esta sala está
  ahora en este túnel»); se añade sin tocar nada más.
- Opcional: miniatura grande (p. ej. 2048 px) de los fondos de mapa, si son muy pesados para la subida del host.
- Probar a mano con varios jugadores a la vez, móvil, cortes de red (el navegador reintenta durante un minuto) y
  cerrar la app del host con gente dentro.
