# Resultados de la fase 0

## Decisión: `webrtc` 0.21.1 (webrtc-rs)

Es la única que conectó **en directo entre dos redes reales** (`srflx → srflx`, sin túnel ni relé). Además reúne ella sola los candidatos (interfaces + STUN), lleva su propio bucle de red y su certificado no caduca. `str0m` funcionó igual de bien en el mismo PC, pero desde otra red no conectó: le faltaría el código de red que hemos tenido que escribir a mano (varias IP, IPv6…), que es justo lo que no se quiere mantener.

Para la app: dependencias `webrtc` 0.21.1 + `rtc` 0.21.1 (certificado), `async-trait`, `bytes`. Tiempos ICE largos (`SettingEngineBuilder::with_ice_timeouts`, ~5 min) para los códigos a mano: con los 25 s de serie el host cerraba antes de que el jugador pegara la respuesta. En Linux, `ring` necesita compilador de C.

## Con navegador

| Librería | Host | Navegador | Modo | Camino | Conectar | Latencia `juego` mediana / p95 | Arrastre 20/s × 10 s | Bajada | Subida | 64 KiB / 256 KiB |
|---|---|---|---|---|---|---|---|---|---|---|
| str0m | Windows | Chrome 157 | mismo PC | host → host | 14 ms | 0,2 / 0,4 ms | 200/200, mediana 0,8 ms | 37,3 MB/s | 23,8 MB/s | vuelve / no vuelve |
| str0m | Windows | Firefox 160 | mismo PC | host → host | 63 ms | 0 / 1 ms | 200/200, mediana 1 ms | 30,6 MB/s | 44,4 MB/s | vuelve / no vuelve |
| webrtc-rs | Windows | Chrome 157 | mismo PC | host → host | 35 ms | 0,2 / 0,3 ms | 200/200, mediana 0,9 ms | 51,6 MB/s | 40,9 MB/s | vuelve / el navegador no lo envía |
| webrtc-rs | Windows | Firefox 160 | mismo PC | host → host | 236 ms | 0 / 1 ms | 200/200, mediana 1 ms | 15,1 MB/s | 55,9 MB/s | vuelve / el navegador no lo envía |
| **webrtc-rs** | **Windows** | **Chrome 157** | **otra red (manual)** | **srflx → srflx** | **410 ms** | **36,9 / 38,1 ms** | **200/200, mediana 37,1 ms** | **7,2 MB/s (60 Mbit/s)** | **1,7 MB/s (14 Mbit/s)** | vuelve / el navegador no lo envía |
| str0m | Windows | Chrome 157 | otra red (manual) | — | **no conectó** | | | | | |
| str0m | Linux | Firefox 146 | mismo PC (2.º intento) | host → host | 3 ms | 0 / 1 ms | 200/200, mediana 1 ms | 40,7 MB/s | 50,3 MB/s | vuelve / no vuelve |
| webrtc-rs | Linux | Firefox 146 | mismo PC (2.º intento) | host → host | 204 ms | 0 / 1 ms | 200/200, mediana 1 ms | 61,5 MB/s | 66,2 MB/s | vuelve / el navegador no lo envía |

- **Otra red**: 37 ms de ida y vuelta entre dos casas reales, sin pérdidas en el arrastre. El túnel de Cloudflare dio 70 ms con los dos extremos **en el mismo PC**, así que en una partida real el directo es bastante más rápido. La subida de 14 Mbit/s es, probablemente, el límite de la línea del compañero.
- **Tamaño de mensaje**: webrtc-rs anuncia `max-message-size` 65 536; el navegador manda 16 y 64 KiB y se niega a 256 KiB con un `TypeError` (falla limpio). str0m anuncia 256 KiB, pero uno de 256 KiB no vuelve. Con trozos de 16 KiB (plan, fase 1) no hay problema en ninguna.
- **Huella**: coincide siempre con la del certificado guardado, y se mantiene al reiniciar.

## Pendiente: primer intento lento en Linux

En Linux, el **primer** intento tras arrancar cada servidor tardó mucho (con webrtc-rs llegó a fallar) y el segundo fue casi instantáneo. Los datos de arriba son de los segundos intentos. Esas pruebas fueron **antes** del arreglo de los tiempos ICE, así que un primer intento de más de 25 s acababa en fallo: eso explica el fallo, pero no la lentitud.

Como pasa con las dos librerías, apunta al sistema o al navegador en su primer uso (nombres mDNS `.local` de Firefox, cortafuegos, DNS/STUN en frío), no a la librería. Para comprobarlo, la página guarda ahora una cronología de cada paso (también si falla) y la consola del servidor escribe la suya: ver «Prueba en frío» en `LEEME.md`. No bloquea la fase 1.

**Primer dato con la cronología (Linux, Firefox 146, webrtc-rs, 17:46 UTC):** la oferta se envió a los 114 ms y la respuesta del host llegó a los **3 120 ms**. Después, conectar tardó solo 0,2 s (ICE `connected` a los 21 ms de aplicar la respuesta, canales abiertos a los 3,3 s). Es decir: la espera la causa el **host al contestar**, y 3 s es justo el tope que puse en el experimento a «reunir candidatos» (`nuevo_peer` espera a `Complete`, que en Linux no llegó a tiempo aunque la respuesta ya traía sus dos `srflx`). No es lentitud de la conexión. Para la app: el host contestará en cuanto tenga el primer `srflx` (+ ~300 ms de gracia) y como mucho a 1,5 s, sin esperar a `Complete`. Falta saber por qué no llega a `Complete` en Linux (¿IPv6 o la segunda consulta STUN?), pero ya no bloquea nada. El primer intento de esa misma tanda (`NetworkError`, 1 ms) fue el servidor no accesible desde la terminal de VS Code, no un fallo de WebRTC.

## Medido automáticamente (Windows 11, 2026-11-12)

| | `webrtc` 0.21.1 (webrtc-rs) | `str0m` 0.25.0 (rust-crypto) |
|---|---|---|
| Compila | Windows y Linux (necesita compilador de C por `ring`) | Windows y Linux, Rust puro |
| Prueba automática (`cargo test`): conecta, eco en los 2 canales, bajada y subida 1 MB | ✅ | ✅ |
| Canal sin orden ni reintentos | ✅ | ✅ |
| Certificado DTLS persistente | ✅ (PEM), válido hasta el año 4096 | ✅ (DER en JSON), válido 1 año |
| Ejecutable release (axum + la librería) | 4,79 MB (≈ +4,2 MB) | 4,56 MB (≈ +4,0 MB) |
| Dependencias (incluida ella) | 239 | 156 |
| Licencias de las dependencias | todas permisivas (MIT, Apache-2.0, BSD, ISC, Unicode, Zlib…): compatibles con GPL-3.0 | ídem |
| Reúne candidatos ICE ella sola | sí | no: socket, IP y STUN a mano (`comun.rs`), una sola IPv4 |
| Bucle de red | lo lleva la librería | lo llevamos nosotros (sans-IO) |

Compilación release de los tres binarios (LTO, 1 unidad): 3 min 53 s. Cliente STUN propio comprobado contra `stun.l.google.com:19302` y `stun.cloudflare.com:3478` (`cargo test -- --ignored`).
