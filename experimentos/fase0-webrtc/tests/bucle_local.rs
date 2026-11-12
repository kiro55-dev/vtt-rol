//! Sin navegador: un cliente `webrtc` en el mismo proceso hace de jugador contra cada servidor. Abre los dos
//! canales (`juego` fiable y `arrastre` sin orden ni reintentos), comprueba el eco, la bajada y la subida, y que
//! la huella de la respuesta es la del certificado guardado (que no cambia al volver a cargarlo).
use std::sync::Arc;
use std::time::Duration;

use bytes::BytesMut;
use fase0_webrtc::{con_str0m, con_webrtcrs};
use tokio::sync::mpsc;
use webrtc::data_channel::{DataChannel, DataChannelEvent, RTCDataChannelInit};
use webrtc::peer_connection::{
    PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCConfigurationBuilder, RTCIceGatheringState,
    RTCSessionDescription,
};
use webrtc::runtime::{default_runtime, Runtime};

struct Cliente {
    reunidos: mpsc::Sender<()>,
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for Cliente {
    async fn on_ice_gathering_state_change(&self, estado: RTCIceGatheringState) {
        if estado == RTCIceGatheringState::Complete {
            let _ = self.reunidos.try_send(());
        }
    }
}

enum Llega {
    Abierto,
    Texto(String),
    Binario(usize),
}

/// Lee los eventos de un canal y los pasa a `tx`.
fn escuchar(rt: &Arc<dyn Runtime>, canal: Arc<dyn DataChannel>, tx: mpsc::UnboundedSender<Llega>) {
    rt.spawn(Box::pin(async move {
        while let Some(ev) = canal.poll().await {
            let _ = match ev {
                DataChannelEvent::OnOpen => tx.send(Llega::Abierto),
                DataChannelEvent::OnMessage(m) if m.is_string => tx.send(Llega::Texto(String::from_utf8_lossy(&m.data).into())),
                DataChannelEvent::OnMessage(m) => tx.send(Llega::Binario(m.data.len())),
                DataChannelEvent::OnClose => break,
                _ => Ok(()),
            };
        }
    }));
}

async fn siguiente(rx: &mut mpsc::UnboundedReceiver<Llega>) -> Llega {
    tokio::time::timeout(Duration::from_secs(15), rx.recv()).await.expect("no llegó a tiempo").expect("canal cerrado")
}

async fn texto(rx: &mut mpsc::UnboundedReceiver<Llega>) -> String {
    loop {
        if let Llega::Texto(t) = siguiente(rx).await {
            return t;
        }
    }
}

/// Hace de jugador contra `responder` (la función de un servidor). Devuelve la huella de la respuesta.
async fn jugar<F, Fut>(responder: F) -> String
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    let rt = default_runtime().unwrap();
    let (reunidos, mut al_reunir) = mpsc::channel(1);
    let pc = PeerConnectionBuilder::new()
        .with_configuration(RTCConfigurationBuilder::new().build())
        .with_handler(Arc::new(Cliente { reunidos }))
        .with_runtime(rt.clone())
        .with_udp_addrs(vec!["0.0.0.0:0".to_string()])
        .build()
        .await
        .unwrap();
    let juego = pc.create_data_channel("juego", None).await.unwrap();
    let sin_orden = RTCDataChannelInit { ordered: false, max_retransmits: Some(0), ..Default::default() };
    let arrastre = pc.create_data_channel("arrastre", Some(sin_orden)).await.unwrap();
    let (tx_j, mut rx_j) = mpsc::unbounded_channel();
    let (tx_a, mut rx_a) = mpsc::unbounded_channel();
    escuchar(&rt, juego.clone(), tx_j);
    escuchar(&rt, arrastre.clone(), tx_a);

    let oferta = pc.create_offer(None).await.unwrap();
    pc.set_local_description(oferta).await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(3), al_reunir.recv()).await;
    let oferta = serde_json::to_string(&pc.local_description().await.unwrap()).unwrap();

    let respuesta = responder(oferta).await.expect("el servidor no contestó la oferta");
    let respuesta: RTCSessionDescription = serde_json::from_str(&respuesta).unwrap();
    let huella = respuesta
        .sdp
        .lines()
        .find_map(|l| l.strip_prefix("a=fingerprint:sha-256 "))
        .expect("la respuesta trae huella")
        .trim()
        .to_uppercase();
    pc.set_remote_description(respuesta).await.unwrap();

    assert!(matches!(siguiente(&mut rx_j).await, Llega::Abierto), "se abre «juego»");
    assert!(matches!(siguiente(&mut rx_a).await, Llega::Abierto), "se abre «arrastre»");

    // Eco por los dos canales
    juego.send_text("hola").await.unwrap();
    assert_eq!(texto(&mut rx_j).await, "hola");
    arrastre.send_text("pieza 3,4").await.unwrap();
    assert_eq!(texto(&mut rx_a).await, "pieza 3,4");

    // Bajada: 1 MB en trozos
    juego.send_text("TAM 1048576").await.unwrap();
    let mut bajados = 0;
    while bajados < 1_048_576 {
        if let Llega::Binario(n) = siguiente(&mut rx_j).await {
            bajados += n;
        }
    }
    assert_eq!(bajados, 1_048_576);

    // Subida: 64 trozos de 16 KiB y la cuenta
    for _ in 0..64 {
        juego.send(BytesMut::from(&[7u8; 16 * 1024][..])).await.unwrap();
    }
    juego.send_text("CUENTA").await.unwrap();
    assert_eq!(texto(&mut rx_j).await, format!("RECIBIDOS {}", 64 * 16 * 1024));

    let _ = pc.close().await;
    huella
}

fn ruta(nombre: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join("fase0-webrtc-tests");
    std::fs::create_dir_all(&d).unwrap();
    let r = d.join(nombre);
    let _ = std::fs::remove_file(&r);
    r
}

#[tokio::test(flavor = "multi_thread")]
async fn webrtcrs_conecta_y_su_huella_es_fija() {
    let r = ruta("cert-webrtcrs.pem");
    let cert = con_webrtcrs::certificado(&r).unwrap();
    assert_eq!(con_webrtcrs::certificado(&r).unwrap().huella, cert.huella, "la misma al volver a cargarlo");
    let huella = jugar(|o| async { con_webrtcrs::nuevo_peer(o, &cert, false).await }).await;
    assert_eq!(huella, cert.huella, "la huella del SDP es la del certificado guardado");
}

#[tokio::test(flavor = "multi_thread")]
async fn str0m_conecta_y_su_huella_es_fija() {
    let r = ruta("cert-str0m.json");
    let cert = con_str0m::certificado(&r).unwrap();
    assert_eq!(con_str0m::certificado(&r).unwrap().huella, cert.huella, "la misma al volver a cargarlo");
    let huella = jugar(|o| async { con_str0m::nuevo_peer(o, &cert, false).await }).await;
    assert_eq!(huella, cert.huella, "la huella del SDP es la del certificado guardado");
}

/// Necesita internet: `cargo test -- --ignored`. Comprueba el cliente STUN propio (str0m no trae).
#[tokio::test]
#[ignore]
async fn stun_devuelve_la_ip_publica() {
    let sock = tokio::net::UdpSocket::bind("0.0.0.0:0").await.unwrap();
    for s in fase0_webrtc::comun::STUN {
        let publica = fase0_webrtc::comun::stun_publica(&sock, s).await;
        println!("{s}: {publica:?}");
        assert!(publica.is_some(), "{s} no contestó");
    }
}
