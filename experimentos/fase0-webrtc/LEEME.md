# Fase 0: prueba de concepto WebRTC

Proyecto aparte (no forma parte de la app) para elegir la librería WebRTC de Rust del host:

- `servidor-webrtcrs`: [`webrtc`](https://crates.io/crates/webrtc) 0.21.1 (webrtc-rs, cripto `ring`).
- `servidor-str0m`: [`str0m`](https://crates.io/crates/str0m) 0.25.0 (cripto 100 % Rust, `rust-crypto`).

Los dos hacen lo mismo: contestan la oferta del navegador, abren dos canales (`juego`, fiable y ordenado; `arrastre`, sin orden ni reintentos) y responden a las pruebas de `pagina.html`.

## Compilar

Hace falta Rust (`rustup`). En Linux, además, un compilador de C (`build-essential` en Debian/Ubuntu, `base-devel` en Arch), que pide `ring`.

```sh
cd experimentos/fase0-webrtc
cargo test                   # prueba automática de las dos librerías (sin navegador)
cargo build --release
```

## Prueba en el mismo PC

```sh
cargo run --release --bin servidor-str0m      # o servidor-webrtcrs
```

1. Abre http://127.0.0.1:8090 en **Chrome** y pulsa **Conectar (mismo PC)**. Tarda unos 30 s.
2. Pulsa **Copiar resultados** y guarda el texto.
3. Repite en **Firefox**.
4. Para el servidor (Ctrl+C) y repite con el otro (`servidor-webrtcrs`).

La primera vez se crea `cert-str0m.json` / `cert-webrtcrs.pem` en esta carpeta: es el certificado DTLS del «host». Al reiniciar se reutiliza y la huella que imprime la consola no cambia.

## Prueba desde otra red (sin abrir puertos)

1. En el host: `cargo run --release --bin servidor-str0m -- --manual`.
2. Pásale a la otra persona el archivo `pagina.html` (lo abre con doble clic, en Chrome o Firefox).
3. Ella pulsa **Modo manual (otra red)**, copia el código y te lo manda (Discord, WhatsApp…).
4. Lo pegas en la consola del host y le devuelves el código de respuesta que sale.
5. Ella lo pega, pulsa **Usar respuesta**, espera ~30 s y te manda lo de **Copiar resultados**.
6. Comprobad que la huella de la tabla es la que imprimió la consola del host.

Si no conecta, los dos estáis detrás de routers que no lo permiten (NAT simétrico/CGNAT): es justo el caso que cubrirá el túnel de respaldo. Apúntalo igualmente.

## Qué hacer con los resultados

Pégale los JSON a Claude (o rellena `RESULTADOS.md`): con eso se elige la librería.
