//! Lo que comparten los dos servidores de prueba: la página, el HTTP local, el modo manual por consola,
//! el comportamiento de los canales, la huella del certificado y un cliente STUN mínimo (str0m no trae).
use std::future::Future;
use std::io::{BufRead, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use axum::routing::{get, post};
use axum::Router;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use sha2::{Digest, Sha256};

pub const PAGINA: &str = include_str!("../pagina.html");
pub const PUERTO: u16 = 8090;
/// Trozo de la bajada (lo que se manda en cada mensaje binario).
pub const TROZO: usize = 16 * 1024;
/// Bytes que se dejan en el búfer de salida de un canal antes de esperar.
pub const MAX_BUFER: usize = 1024 * 1024;
pub const STUN: [&str; 2] = ["stun.l.google.com:19302", "stun.cloudflare.com:3478"];

pub type Futuro<T> = Pin<Box<dyn Future<Output = T> + Send>>;
/// Recibe la oferta del navegador (JSON `{type, sdp}`) y devuelve la respuesta (mismo formato).
pub type Responder = Arc<dyn Fn(String) -> Futuro<Result<String, String>> + Send + Sync>;

/// Lo que llega por un canal.
pub enum Mensaje {
    Texto(String),
    Binario(usize),
}

/// Lo que hay que hacer con lo que llegó.
#[derive(Debug, PartialEq)]
pub enum Respuesta {
    Nada,
    Texto(String),
    /// Mandar tantos bytes en binario, en trozos de `TROZO`.
    Bajada(usize),
}

/// Estado de un canal: bytes binarios recibidos desde el último `CUENTA` (la prueba de subida).
#[derive(Default)]
pub struct EstadoCanal {
    pub recibidos: u64,
}

/// El comportamiento de los canales, igual para las dos librerías:
///  * `TAM <n>` → mandar `n` bytes en binario (bajada);
///  * binario → se cuenta; `CUENTA` → `RECIBIDOS <n>` (subida);
///  * cualquier otro texto → se devuelve tal cual (eco: latencia y tamaños).
pub fn procesar(estado: &mut EstadoCanal, m: Mensaje) -> Respuesta {
    match m {
        Mensaje::Binario(n) => {
            estado.recibidos += n as u64;
            Respuesta::Nada
        }
        Mensaje::Texto(t) => {
            if let Some(n) = t.strip_prefix("TAM ").and_then(|n| n.trim().parse::<usize>().ok()) {
                Respuesta::Bajada(n.min(200 * 1024 * 1024))
            } else if t == "CUENTA" {
                let n = std::mem::take(&mut estado.recibidos);
                Respuesta::Texto(format!("RECIBIDOS {n}"))
            } else {
                Respuesta::Texto(t)
            }
        }
    }
}

/// Huella SHA-256 de un certificado DER, como en el SDP: `AB:CD:…` en mayúsculas.
pub fn huella_de_der(der: &[u8]) -> String {
    Sha256::digest(der).iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}

/// La IP de la interfaz que sale a internet (sin enviar nada: `connect` en UDP solo elige ruta).
/// Firefox no acepta 127.0.0.1 como candidato, así que hace falta una de verdad.
pub fn ip_local() -> IpAddr {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").and_then(|s| s.connect("8.8.8.8:80").map(|_| s));
    s.and_then(|s| s.local_addr()).map(|a| a.ip()).unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST))
}

/// Pregunta a un servidor STUN cómo nos ve desde fuera (RFC 5389, Binding Request) usando ESTE socket, que es
/// el que usará la conexión. `None` si no contesta en ~3 s.
pub async fn stun_publica(sock: &tokio::net::UdpSocket, servidor: &str) -> Option<SocketAddr> {
    let destino = tokio::net::lookup_host(servidor).await.ok()?.find(|a| a.is_ipv4())?;
    let semilla = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
    let id: [u8; 12] = Sha256::digest(semilla.to_le_bytes())[..12].try_into().ok()?;
    let mut peticion = vec![0x00, 0x01, 0x00, 0x00, 0x21, 0x12, 0xA4, 0x42];
    peticion.extend_from_slice(&id);
    let mut buf = [0u8; 1500];
    for _ in 0..3 {
        sock.send_to(&peticion, destino).await.ok()?;
        let Ok(Ok((n, _))) = tokio::time::timeout(Duration::from_secs(1), sock.recv_from(&mut buf)).await else { continue };
        let r = &buf[..n];
        if n < 20 || r[0..2] != [0x01, 0x01] || r[8..20] != id {
            continue;
        }
        let mut i = 20;
        while i + 4 <= n {
            let tipo = u16::from_be_bytes([r[i], r[i + 1]]);
            let largo = u16::from_be_bytes([r[i + 2], r[i + 3]]) as usize;
            let v = r.get(i + 4..i + 4 + largo)?;
            // XOR-MAPPED-ADDRESS (0x0020), IPv4
            if tipo == 0x0020 && largo >= 8 && v[1] == 0x01 {
                let puerto = u16::from_be_bytes([v[2], v[3]]) ^ 0x2112;
                let ip = Ipv4Addr::new(v[4] ^ 0x21, v[5] ^ 0x12, v[6] ^ 0xA4, v[7] ^ 0x42);
                return Some(SocketAddr::new(IpAddr::V4(ip), puerto));
            }
            i += 4 + largo.div_ceil(4) * 4;
        }
    }
    None
}

/// Arranca el servidor de pruebas: HTTP local en `127.0.0.1:8090`, o con `--manual` por consola (pegar la
/// oferta en base64 y copiar la respuesta), para probar desde otra red sin abrir puertos.
pub async fn servir(libreria: &'static str, huella: String, responder: Responder) {
    println!("Librería: {libreria}");
    println!("Huella del certificado DTLS: {huella}");
    if std::env::args().any(|a| a == "--manual") {
        return manual(responder).await;
    }
    let r = responder.clone();
    let app = Router::new()
        .route("/", get(|| async { axum::response::Html(PAGINA) }))
        .route("/huella", get(move || async move { huella }))
        .route("/info", get(move || async move { libreria }))
        .route(
            "/oferta",
            post(move |cuerpo: String| {
                let r = r.clone();
                async move {
                    match r(cuerpo).await {
                        Ok(json) => (axum::http::StatusCode::OK, json),
                        Err(e) => {
                            eprintln!("Error con la oferta: {e}");
                            (axum::http::StatusCode::BAD_REQUEST, e)
                        }
                    }
                }
            }),
        );
    let escucha = tokio::net::TcpListener::bind(("127.0.0.1", PUERTO)).await.expect("¿el puerto 8090 está ocupado?");
    println!("Abre http://127.0.0.1:{PUERTO} en Chrome o Firefox. Ctrl+C para salir.");
    axum::serve(escucha, app).await.unwrap();
}

async fn manual(responder: Responder) {
    println!("Modo manual. Abre pagina.html en el navegador (también en otra máquina), pulsa «Modo manual»,");
    println!("copia su código y pégalo aquí (una línea). Ctrl+C para salir.");
    loop {
        print!("\nOferta> ");
        let _ = std::io::stdout().flush();
        let linea = tokio::task::spawn_blocking(|| {
            let mut l = String::new();
            std::io::stdin().lock().read_line(&mut l).map(|_| l)
        })
        .await;
        let Ok(Ok(linea)) = linea else { return };
        let linea = linea.trim();
        if linea.is_empty() {
            continue;
        }
        let oferta = match B64.decode(linea).ok().and_then(|b| String::from_utf8(b).ok()) {
            Some(o) => o,
            None => {
                println!("Eso no es un código válido");
                continue;
            }
        };
        match responder(oferta).await {
            Ok(json) => println!("\nRespuesta (cópiala en la página):\n{}", B64.encode(json)),
            Err(e) => println!("Error: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_comportamiento_de_los_canales() {
        let mut e = EstadoCanal::default();
        assert_eq!(procesar(&mut e, Mensaje::Texto("hola".into())), Respuesta::Texto("hola".into()));
        assert_eq!(procesar(&mut e, Mensaje::Texto("TAM 100".into())), Respuesta::Bajada(100));
        assert_eq!(procesar(&mut e, Mensaje::Binario(10)), Respuesta::Nada);
        assert_eq!(procesar(&mut e, Mensaje::Binario(5)), Respuesta::Nada);
        assert_eq!(procesar(&mut e, Mensaje::Texto("CUENTA".into())), Respuesta::Texto("RECIBIDOS 15".into()));
        assert_eq!(procesar(&mut e, Mensaje::Texto("CUENTA".into())), Respuesta::Texto("RECIBIDOS 0".into()));
    }

    #[test]
    fn la_huella_tiene_el_formato_del_sdp() {
        let h = huella_de_der(b"x");
        assert_eq!(h.len(), 32 * 3 - 1);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase() || c == ':'));
    }
}
