//! Servidor de pruebas con la librería str0m. Ver LEEME.md.
use std::path::Path;
use std::sync::Arc;

use fase0_webrtc::{comun, con_str0m};

#[tokio::main]
async fn main() {
    let cert = con_str0m::certificado(Path::new("cert-str0m.json")).expect("certificado DTLS");
    let huella = cert.huella.clone();
    let cert = Arc::new(cert);
    let responder: comun::Responder = Arc::new(move |oferta| {
        let cert = cert.clone();
        Box::pin(async move { con_str0m::nuevo_peer(oferta, &cert, true).await })
    });
    comun::servir("str0m 0.25.0 (rust-crypto)", huella, responder).await;
}
