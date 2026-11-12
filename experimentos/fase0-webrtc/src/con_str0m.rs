//! Candidata 2: `str0m` 0.25 (sans-IO, cripto 100 % Rust). La librería no toca la red: aquí se crea el socket
//! UDP, se averigua la IP pública con STUN (no lo hace ella), se añaden los candidatos y se lleva el bucle.
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::Once;
use std::time::Instant;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use str0m::change::SdpOffer;
use str0m::channel::ChannelId;
use str0m::config::DtlsCert;
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, IceConnectionState, Input, Output, Rtc, RtcConfig};
use tokio::net::UdpSocket;

use crate::comun::{self, EstadoCanal, Mensaje, Respuesta, MAX_BUFER, TROZO};

pub struct Certificado {
    cert: DtlsCert,
    pub huella: String,
}

static CRIPTO: Once = Once::new();

fn instalar_cripto() {
    CRIPTO.call_once(|| str0m::crypto::from_feature_flags().install_process_default());
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CertGuardado {
    certificado: String,
    clave: String,
}

/// El certificado DTLS de `ruta` (JSON con el DER en base64), o uno nuevo que se guarda ahí.
pub fn certificado(ruta: &Path) -> Result<Certificado, String> {
    instalar_cripto();
    let cert = match std::fs::read_to_string(ruta) {
        Ok(texto) => {
            let g: CertGuardado = serde_json::from_str(&texto).map_err(|e| e.to_string())?;
            DtlsCert {
                certificate: B64.decode(g.certificado).map_err(|e| e.to_string())?,
                private_key: B64.decode(g.clave).map_err(|e| e.to_string())?,
            }
        }
        Err(_) => {
            let p = str0m::crypto::from_feature_flags();
            let c = p.dtls_provider.generate_certificate().ok_or("no se pudo generar el certificado")?;
            let g = CertGuardado { certificado: B64.encode(&c.certificate), clave: B64.encode(&c.private_key) };
            std::fs::write(ruta, serde_json::to_string_pretty(&g).unwrap()).map_err(|e| e.to_string())?;
            c
        }
    };
    let huella = comun::huella_de_der(&cert.certificate);
    Ok(Certificado { cert, huella })
}

/// Contesta una oferta y lanza el bucle de la conexión. `stun = false` en los tests (sin red).
pub async fn nuevo_peer(oferta_json: String, cert: &Certificado, stun: bool) -> Result<String, String> {
    instalar_cripto();
    let oferta: SdpOffer = serde_json::from_str(&oferta_json).map_err(|e| format!("oferta: {e}"))?;
    let mut config = RtcConfig::new().set_dtls_cert(cert.cert.clone());
    // Paciencia: en el modo manual el otro lado tarda minutos en pegar la respuesta. Con los 9 reintentos de
    // serie (~20 s) la conexión se daba por perdida antes de que el navegador empezara.
    config.set_max_stun_retransmits(100);
    let mut rtc = config.build(Instant::now());

    let sock = UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)).await.map_err(|e| e.to_string())?;
    let local = SocketAddr::new(comun::ip_local(), sock.local_addr().map_err(|e| e.to_string())?.port());
    rtc.add_local_candidate(Candidate::host(local, "udp").map_err(|e| format!("candidato: {e}"))?);
    if stun {
        for s in comun::STUN {
            if let Some(publica) = comun::stun_publica(&sock, s).await {
                if publica != local {
                    if let Ok(c) = Candidate::server_reflexive(publica, local, "udp") {
                        rtc.add_local_candidate(c);
                    }
                }
                break;
            }
        }
    }
    let respuesta = rtc.sdp_api().accept_offer(oferta).map_err(|e| format!("respuesta: {e}"))?;
    let json = serde_json::to_string(&respuesta).map_err(|e| e.to_string())?;
    tokio::spawn(async move {
        if let Err(e) = bucle(rtc, sock, local).await {
            println!("[str0m] fin de la conexión: {e}");
        }
    });
    Ok(json)
}

struct Canal {
    etiqueta: String,
    estado: EstadoCanal,
    /// Bytes de bajada aún por meter en el canal.
    pendiente: usize,
}

async fn bucle(mut rtc: Rtc, sock: UdpSocket, local: SocketAddr) -> Result<(), String> {
    let mut buf = vec![0u8; 2048];
    let mut canales: HashMap<ChannelId, Canal> = HashMap::new();
    let trozo = vec![b'X'; TROZO];
    loop {
        // 1. Sacar todo lo que tenga que salir hasta que pida esperar
        let hasta = loop {
            match rtc.poll_output().map_err(|e| e.to_string())? {
                Output::Timeout(t) => break t,
                Output::Transmit(t) => {
                    let _ = sock.send_to(&t.contents, t.destination).await;
                }
                Output::Event(ev) => match ev {
                    Event::IceConnectionStateChange(s) => {
                        println!("[str0m] ICE: {s:?}");
                        if s == IceConnectionState::Disconnected {
                            return Ok(());
                        }
                    }
                    Event::Connected => println!("[str0m] conectado"),
                    Event::ChannelOpen(id, etiqueta) => {
                        println!("[str0m] canal «{etiqueta}»");
                        canales.insert(id, Canal { etiqueta, estado: EstadoCanal::default(), pendiente: 0 });
                    }
                    Event::ChannelData(d) => {
                        let Some(c) = canales.get_mut(&d.id) else { continue };
                        let mensaje = if d.binary {
                            Mensaje::Binario(d.data.len())
                        } else {
                            Mensaje::Texto(String::from_utf8_lossy(&d.data).into_owned())
                        };
                        match comun::procesar(&mut c.estado, mensaje) {
                            Respuesta::Nada => {}
                            Respuesta::Texto(t) => {
                                if let Some(mut ch) = rtc.channel(d.id) {
                                    if let Err(e) = ch.write(false, t.as_bytes()) {
                                        println!("[str0m] no se pudo enviar por «{}»: {e}", c.etiqueta);
                                    }
                                }
                            }
                            Respuesta::Bajada(n) => c.pendiente += n,
                        }
                    }
                    Event::ChannelClose(id) => {
                        canales.remove(&id);
                    }
                    _ => {}
                },
            }
        };

        // 2. Meter bajada mientras el búfer de cada canal tenga sitio
        let mut escrito = false;
        for (id, c) in canales.iter_mut().filter(|(_, c)| c.pendiente > 0) {
            let Some(mut ch) = rtc.channel(*id) else { continue };
            while c.pendiente > 0 && ch.buffered_amount() < MAX_BUFER {
                let k = c.pendiente.min(TROZO);
                match ch.write(true, &trozo[..k]) {
                    Ok(true) => {
                        c.pendiente -= k;
                        escrito = true;
                    }
                    _ => break,
                }
            }
        }
        if escrito {
            continue;
        }

        // 3. Esperar un paquete o el siguiente aviso de tiempo
        let espera = hasta.saturating_duration_since(Instant::now());
        if espera.is_zero() {
            rtc.handle_input(Input::Timeout(Instant::now())).map_err(|e| e.to_string())?;
            continue;
        }
        tokio::select! {
            r = sock.recv_from(&mut buf) => {
                let (n, origen) = r.map_err(|e| e.to_string())?;
                let Ok(contenido) = buf[..n].try_into() else { continue };
                let entrada = Input::Receive(Instant::now(), Receive { proto: Protocol::Udp, source: origen, destination: local, contents: contenido });
                if rtc.accepts(&entrada) {
                    rtc.handle_input(entrada).map_err(|e| e.to_string())?;
                }
            }
            _ = tokio::time::sleep(espera) => {
                rtc.handle_input(Input::Timeout(Instant::now())).map_err(|e| e.to_string())?;
            }
        }
        if !rtc.is_alive() {
            return Ok(());
        }
    }
}
