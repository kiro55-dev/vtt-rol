# Resultados de la fase 0

## Medido automáticamente (Windows 11, 2026-11-12)

| | `webrtc` 0.21.1 (webrtc-rs) | `str0m` 0.25.0 (rust-crypto) |
|---|---|---|
| Compila en Windows | sí (necesita compilador de C por `ring`) | sí, Rust puro |
| Prueba automática (`cargo test`): conecta, eco en los 2 canales, bajada 1 MB, subida 1 MB | ✅ | ✅ |
| Canal sin orden ni reintentos (`arrastre`) | ✅ | ✅ |
| Certificado DTLS persistente, misma huella al reiniciar y la del SDP | ✅ (PEM) | ✅ (DER en JSON) |
| Validez del certificado generado | hasta el año 4096 | **1 año** (habría que crearlo nosotros con más validez) |
| Ejecutable release (axum + la librería) | 4,79 MB | 4,56 MB |
| Lo que añade frente a `base` (axum + tokio, 0,55 MB) | ≈ +4,2 MB | ≈ +4,0 MB |
| Dependencias (incluida ella) | 239 | 156 |
| Licencias de todas las dependencias | todas permisivas (MIT, Apache-2.0, BSD, ISC, Unicode, Zlib…): compatibles con GPL-3.0 | ídem |
| Reúne candidatos ICE él solo (interfaces + STUN) | sí | **no**: hay que crear el socket, elegir la IP y preguntar a STUN a mano (hecho en `comun.rs`, ~40 líneas; solo una interfaz IPv4) |
| Bucle de red | lo lleva la librería (su propio grupo de hilos) | lo llevamos nosotros (sans-IO: `poll_output`/`handle_input`) |
| Tamaño máximo de mensaje recibido | **16 KiB** según su documentación (`OnMessage can currently receive messages up to 16384 bytes`); a confirmar con la página | a confirmar con la página |

Compilación release de los tres binarios a la vez (LTO, 1 unidad): 3 min 53 s.

Cliente STUN propio comprobado contra `stun.l.google.com:19302` y `stun.cloudflare.com:3478`: los dos devuelven la IP pública (`cargo test -- --ignored`).

## Con navegador (rellenar con los JSON de «Copiar resultados»)

| Librería | Sistema host | Navegador | Modo | Conecta | Camino | Latencia `juego` mediana / p95 | Arrastre: % perdidos, mediana | Bajada MB/s | Subida MB/s | 64 KiB / 256 KiB |
|---|---|---|---|---|---|---|---|---|---|---|
| webrtc-rs | Windows | Chrome | local | | | | | | | |
| webrtc-rs | Windows | Firefox | local | | | | | | | |
| str0m | Windows | Chrome | local | | | | | | | |
| str0m | Windows | Firefox | local | | | | | | | |
| webrtc-rs | Linux | Chrome | local | | | | | | | |
| webrtc-rs | Linux | Firefox | local | | | | | | | |
| str0m | Linux | Chrome | local | | | | | | | |
| str0m | Linux | Firefox | local | | | | | | | |
| (la mejor) | Windows | Chrome | otra red | | | | | | | |

## Criterios

Imprescindibles: conecta con Chrome y Firefox en Windows y Linux; el canal sin reintentos funciona; certificado persistente; compila en Windows y Linux sin OpenSSL/CMake/NASM; licencias compatibles con GPL-3.0.
Después: estabilidad, latencia y caudal, tamaño, y lo fácil que es de encajar en `red/transporte.rs`.

## Valoración provisional (antes de los navegadores)

Las dos cumplen todo lo que se ha podido comprobar sin navegador. Diferencias:

- **webrtc-rs**: más cómoda (reúne candidatos, lleva la red, API asíncrona con eventos), certificado sin caducidad práctica. Más dependencias (239). Es una versión reescrita hace poco (0.20/0.21): riesgo de madurez. Su límite de 16 KiB por mensaje recibido no importa porque el plan ya trocea en 16 KiB.
- **str0m**: menos dependencias y Rust puro (nada que compilar en C), pero más código propio: socket, IP local, STUN, bucle y temporizadores. Solo se probó una interfaz IPv4 (los PC con varias redes o IPv6 necesitarían más trabajo), y su certificado caduca al año.

Si los navegadores no muestran diferencias, la recomendación es **webrtc-rs** por el menor código propio que mantener, con la fragmentación de la fase 1 cubriendo su límite de 16 KiB.
