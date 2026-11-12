//! Candidata 1: `webrtc` 0.21 (webrtc-rs). Reúne sus propios candidatos (host y STUN) y lleva su bucle de red.
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use bytes::BytesMut;
use rtc::crypto::{self, SignatureScheme};
use rtc::peer_connection::certificate::CertificateParams;
use tokio::sync::mpsc;
use webrtc::data_channel::{DataChannel, DataChannelEvent};
use webrtc::peer_connection::{
    PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCCertificate, RTCConfigurationBuilder,
    RTCIceGatheringState, RTCIceServer, RTCPeerConnectionState, RTCSessionDescription,
};
use webrtc::runtime::{default_runtime, Runtime};

use crate::comun::{self, EstadoCanal, Mensaje, Respuesta, MAX_BUFER, TROZO};

pub struct Certificado {
    cert: RTCCertificate,
    pub huella: String,
}

/// El certificado DTLS de `ruta` (PEM), o uno nuevo que se guarda ahí: la huella no cambia al reiniciar.
pub fn certificado(ruta: &Path) -> Result<Certificado, String> {
    let proveedor = crypto::default_provider().map_err(|e| format!("cripto: {e:?}"))?;
    let cert = match std::fs::read_to_string(ruta) {
        Ok(pem) => RTCCertificate::from_pem(&pem, proveedor.crypto()).map_err(|e| format!("PEM: {e}"))?,
        Err(_) => {
            let params = CertificateParams::new(vec!["openroll".to_owned()]).map_err(|e| format!("{e:?}"))?;
            let c = RTCCertificate::generate(proveedor.crypto(), SignatureScheme::EcdsaP256Sha256, params)
                .map_err(|e| format!("generar: {e}"))?;
            std::fs::write(ruta, c.serialize_pem().map_err(|e| format!("{e}"))?).map_err(|e| e.to_string())?;
            c
        }
    };
    let huellas = cert.get_fingerprints(proveedor.crypto()).map_err(|e| format!("{e}"))?;
    let huella = huellas.first().ok_or("sin huella")?.value.to_uppercase();
    Ok(Certificado { cert, huella })
}

struct Manejador {
    runtime: Arc<dyn Runtime>,
    reunidos: mpsc::Sender<()>,
    fin: mpsc::Sender<()>,
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for Manejador {
    async fn on_ice_gathering_state_change(&self, estado: RTCIceGatheringState) {
        if estado == RTCIceGatheringState::Complete {
            let _ = self.reunidos.try_send(());
        }
    }

    async fn on_connection_state_change(&self, estado: RTCPeerConnectionState) {
        println!("[webrtc-rs] conexión: {estado}");
        if matches!(estado, RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed | RTCPeerConnectionState::Disconnected) {
            let _ = self.fin.try_send(());
        }
    }

    async fn on_data_channel(&self, canal: Arc<dyn DataChannel>) {
        let rt = self.runtime.clone();
        self.runtime.spawn(Box::pin(atender_canal(canal, rt)));
    }
}

async fn atender_canal(canal: Arc<dyn DataChannel>, rt: Arc<dyn Runtime>) {
    let etiqueta = canal.label().await.unwrap_or_default();
    println!("[webrtc-rs] canal «{etiqueta}» (ordenado: {:?}, reintentos: {:?})", canal.ordered().await, canal.max_retransmits().await);
    let mut estado = EstadoCanal::default();
    while let Some(ev) = canal.poll().await {
        match ev {
            DataChannelEvent::OnMessage(m) => {
                let mensaje = if m.is_string {
                    Mensaje::Texto(String::from_utf8_lossy(&m.data).into_owned())
                } else {
                    Mensaje::Binario(m.data.len())
                };
                match comun::procesar(&mut estado, mensaje) {
                    Respuesta::Nada => {}
                    Respuesta::Texto(t) => {
                        if let Err(e) = canal.send_text(&t).await {
                            println!("[webrtc-rs] no se pudo enviar por «{etiqueta}»: {e}");
                        }
                    }
                    // En otra tarea, para no dejar de atender el canal mientras se manda
                    Respuesta::Bajada(n) => {
                        let c = canal.clone();
                        rt.spawn(Box::pin(async move {
                            let trozo = vec![b'X'; TROZO];
                            let mut quedan = n;
                            while quedan > 0 {
                                let k = quedan.min(TROZO);
                                // Con límite de búfer configurado, `send` espera a que haya sitio
                                if c.send(BytesMut::from(&trozo[..k])).await.is_err() {
                                    return;
                                }
                                quedan -= k;
                            }
                        }));
                    }
                }
            }
            DataChannelEvent::OnClose => break,
            _ => {}
        }
    }
    println!("[webrtc-rs] canal «{etiqueta}» cerrado");
}

/// Contesta una oferta y deja la conexión viva hasta que se corte. `stun = false` en los tests (sin red).
pub async fn nuevo_peer(oferta_json: String, cert: &Certificado, stun: bool) -> Result<String, String> {
    let runtime = default_runtime().ok_or("sin runtime")?;
    let (reunidos, mut al_reunir) = mpsc::channel(1);
    let (fin, mut al_fin) = mpsc::channel(1);
    let servidores = if stun {
        vec![RTCIceServer { urls: comun::STUN.iter().map(|s| format!("stun:{s}")).collect(), ..Default::default() }]
    } else {
        vec![]
    };
    let config = RTCConfigurationBuilder::new().with_ice_servers(servidores).with_certificates(vec![cert.cert.clone()]).build();
    let pc = PeerConnectionBuilder::new()
        .with_configuration(config)
        .with_handler(Arc::new(Manejador { runtime: runtime.clone(), reunidos, fin }))
        .with_runtime(runtime)
        .with_udp_addrs(vec!["0.0.0.0:0".to_string()])
        .with_data_channel_send_buffer_limit(MAX_BUFER)
        .build()
        .await
        .map_err(|e| format!("crear: {e}"))?;
    let oferta: RTCSessionDescription = serde_json::from_str(&oferta_json).map_err(|e| format!("oferta: {e}"))?;
    pc.set_remote_description(oferta).await.map_err(|e| format!("remota: {e}"))?;
    let respuesta = pc.create_answer(None).await.map_err(|e| format!("respuesta: {e}"))?;
    pc.set_local_description(respuesta).await.map_err(|e| format!("local: {e}"))?;
    // Sin trickle: se espera a tener los candidatos (como mucho 3 s) y se manda todo de una vez
    let _ = tokio::time::timeout(Duration::from_secs(3), al_reunir.recv()).await;
    let local = pc.local_description().await.ok_or("sin descripción local")?;
    let json = serde_json::to_string(&local).map_err(|e| e.to_string())?;
    tokio::spawn(async move {
        let _ = al_fin.recv().await;
        let _ = pc.close().await;
    });
    Ok(json)
}
