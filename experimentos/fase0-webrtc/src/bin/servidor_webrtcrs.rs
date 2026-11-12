//! Servidor de pruebas con la librería webrtcrs. Ver LEEME.md.
use std::path::Path;
use std::sync::Arc;

use fase0_webrtc::{comun, con_webrtcrs};

#[tokio::main]
async fn main() {
    let cert = con_webrtcrs::certificado(Path::new("cert-webrtcrs.pem")).expect("certificado DTLS");
    let huella = cert.huella.clone();
    let cert = Arc::new(cert);
    let responder: comun::Responder = Arc::new(move |oferta| {
        let cert = cert.clone();
        Box::pin(async move { con_webrtcrs::nuevo_peer(oferta, &cert, true).await })
    });
    comun::servir("webrtc-rs 0.21.1 (ring)", huella, responder).await;
}
