// src-tauri/src/red/webrtc.rs
//
// La conexión DIRECTA con un jugador (WebRTC, punto a punto), del lado del host. En esta fase la señalización es a
// mano: el jugador le pasa al host el código de su oferta, `responder` lo comprueba y devuelve el código de la
// respuesta. Cuando los tres canales que abre el navegador están abiertos, se arma un `Transporte` y se lanza el mismo
// `servidor::atender` que atiende a los del túnel: saludo, contraseña, permisos y topes son los mismos.
//
// Canales (los abre el navegador):
//  * `juego`     fiable y ordenado: los mensajes del protocolo, en trozos de ≤ 16 KiB (`red::trozos`).
//  * `imagenes`  fiable y ordenado: los trozos binarios de imagen (aparte, para no retrasar al juego).
//  * `arrastre`  sin orden ni reintentos: el arrastre en vivo (`Trama::Efimero`). Si se pierde uno, el siguiente lo sustituye.
//
// Seguridad: el código va cifrado con el secreto del enlace (`red::codigo`), caduca a los 10 minutos y no se acepta dos
// veces; el certificado DTLS es el de la sala y su huella va en el enlace, así que el navegador solo habla con quien
// tiene esa clave. Como mucho `MAX_PEERS_A_MEDIAS` ofertas contestadas sin abrirse.
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use bytes::BytesMut;
use rtc::peer_connection::certificate::RTCCertificate;
use tokio::sync::mpsc;
use webrtc::data_channel::{DataChannel, DataChannelEvent};
use webrtc::peer_connection::{
    PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCConfigurationBuilder, RTCIceCandidateType,
    RTCIceGatheringState, RTCIceServer, RTCPeerConnectionIceEvent, RTCPeerConnectionState, RTCSessionDescription,
    SettingEngineBuilder,
};
use webrtc::runtime::{default_runtime, Runtime};

use crate::red::codigo::{self, Contenido};
use crate::red::servidor;
use crate::red::transporte::{Trama, Transporte, CAPACIDAD};
use crate::red::trozos::{trocear, Recomponer};
use crate::red::Red;

/// Ofertas contestadas cuyos canales aún no se han abierto, a la vez.
pub const MAX_PEERS_A_MEDIAS: usize = 8;
/// Un código no vale pasados estos milisegundos (ni si dice ser del futuro).
const EDAD_MAX_CODIGO_MS: u64 = 10 * 60 * 1000;
const ADELANTO_MAX_MS: u64 = 60 * 1000;
/// Para contestar: en cuanto hay un candidato público (`srflx`) + esta gracia, o a este tope. No se espera a que la
/// recogida de candidatos «termine» (en Linux, con la librería, a veces no llega a tiempo: fase 0).
const GRACIA_SRFLX: Duration = Duration::from_millis(300);
const TOPE_RESPUESTA: Duration = Duration::from_millis(1500);
/// Los tres canales tienen que abrirse en este tiempo desde que se contesta (si no, se descarta).
const TIEMPO_ABRIRSE: Duration = Duration::from_secs(10 * 60);
/// Un mensaje del protocolo no puede ocupar más (igual que el WebSocket).
const MAX_MENSAJE: usize = 512 * 1024;
/// Lo que se deja en el búfer de salida de un canal antes de esperar (contrapresión).
const MAX_BUFER_CANAL: usize = 1024 * 1024;
/// Servidores STUN (solo dicen cuál es tu IP pública; no ven los datos).
const STUN: [&str; 2] = ["stun:stun.l.google.com:19302", "stun:stun.cloudflare.com:3478"];
const CANALES: [&str; 3] = ["juego", "imagenes", "arrastre"];

/// Una plaza de «conexión directa a medio hacer»; se libera sola.
pub struct PlazaPeer(Arc<Red>);

impl Drop for PlazaPeer {
    fn drop(&mut self) {
        self.0.peers_a_medias.fetch_sub(1, Ordering::Relaxed);
    }
}

impl Red {
    /// El secreto del enlace y el certificado de la sala abierta, si va en peer to peer.
    fn datos_directo(&self) -> Result<(String, RTCCertificate), String> {
        let dentro = self.dentro();
        let s = dentro.as_ref().ok_or("La sala está cerrada")?;
        if !s.p2p() {
            return Err("La conexión directa solo está en «Peer to peer» (en red local ya llegan directos)".into());
        }
        let cert = s.sala.certificado().ok_or("La sala no tiene certificado para la conexión directa")?;
        Ok((s.sala.secreto.clone(), cert))
    }

    /// Anota el `n` de un código. `false` si ya se había usado.
    fn anotar_codigo(&self, n: &str) -> bool {
        let mut dentro = self.dentro();
        let Some(s) = dentro.as_mut() else { return false };
        if s.codigos_vistos.iter().any(|x| x == n) {
            return false;
        }
        s.codigos_vistos.push_back(n.to_string());
        while s.codigos_vistos.len() > 256 {
            s.codigos_vistos.pop_front();
        }
        true
    }

    fn plaza_peer(self: &Arc<Self>) -> Result<PlazaPeer, String> {
        let antes = self.peers_a_medias.fetch_add(1, Ordering::Relaxed);
        let plaza = PlazaPeer(self.clone());
        if antes >= MAX_PEERS_A_MEDIAS {
            return Err("Hay demasiadas conexiones directas a medias: espera a que terminen o se descarten".into());
        }
        Ok(plaza)
    }
}

/// Lo que pasa en un canal.
enum Ev {
    Abierto(String, Arc<dyn DataChannel>),
    Mensaje(String, bool, Vec<u8>),
    /// Se cerró un canal o la conexión: se acaba todo.
    Fin,
}

struct Manejador {
    runtime: Arc<dyn Runtime>,
    eventos: mpsc::UnboundedSender<Ev>,
    srflx: mpsc::Sender<()>,
    completo: mpsc::Sender<()>,
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for Manejador {
    async fn on_ice_candidate(&self, evento: RTCPeerConnectionIceEvent) {
        if evento.candidate.typ == RTCIceCandidateType::Srflx {
            let _ = self.srflx.try_send(());
        }
    }

    async fn on_ice_gathering_state_change(&self, estado: RTCIceGatheringState) {
        if estado == RTCIceGatheringState::Complete {
            let _ = self.completo.try_send(());
        }
    }

    async fn on_connection_state_change(&self, estado: RTCPeerConnectionState) {
        if matches!(estado, RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed | RTCPeerConnectionState::Disconnected) {
            let _ = self.eventos.send(Ev::Fin);
        }
    }

    async fn on_data_channel(&self, canal: Arc<dyn DataChannel>) {
        let eventos = self.eventos.clone();
        self.runtime.spawn(Box::pin(async move {
            let etiqueta = canal.label().await.unwrap_or_default();
            // Un canal creado por el navegador llega ya abierto
            let _ = eventos.send(Ev::Abierto(etiqueta.clone(), canal.clone()));
            while let Some(ev) = canal.poll().await {
                match ev {
                    DataChannelEvent::OnMessage(m) => {
                        if eventos.send(Ev::Mensaje(etiqueta.clone(), m.is_string, m.data.to_vec())).is_err() {
                            return;
                        }
                    }
                    DataChannelEvent::OnClose => break,
                    _ => {}
                }
            }
            let _ = eventos.send(Ev::Fin);
        }));
    }
}

/// Contesta la oferta de un jugador. `Ok` con el código de la respuesta (para pasárselo). La conexión sigue sola.
pub async fn responder(red: &Arc<Red>, codigo: &str) -> Result<String, String> {
    responder_con(red, codigo, true).await
}

/// `stun = false` en los tests (sin red).
pub(crate) async fn responder_con(red: &Arc<Red>, codigo: &str, stun: bool) -> Result<String, String> {
    let (secreto, certificado) = red.datos_directo()?;
    let contenido = codigo::descifrar(&secreto, codigo)?;
    if contenido.tipo != "oferta" {
        return Err("Ese código no es el de un jugador (¿has pegado una respuesta?)".into());
    }
    let ahora = codigo::ahora_ms();
    if ahora.saturating_sub(contenido.t) > EDAD_MAX_CODIGO_MS || contenido.t > ahora + ADELANTO_MAX_MS {
        return Err("Ese código es viejo: que el jugador genere otro".into());
    }
    if contenido.n.len() > 64 || !red.anotar_codigo(&contenido.n) {
        return Err("Ese código ya se usó: que el jugador genere otro".into());
    }
    let plaza = red.plaza_peer()?;

    let runtime = default_runtime().ok_or("No hay runtime para la conexión directa")?;
    let (eventos, recibidos) = mpsc::unbounded_channel();
    let (srflx_tx, mut srflx) = mpsc::channel(1);
    let (completo_tx, mut completo) = mpsc::channel(1);
    let servidores = if stun {
        vec![RTCIceServer { urls: STUN.iter().map(|s| s.to_string()).collect(), ..Default::default() }]
    } else {
        vec![]
    };
    let config = RTCConfigurationBuilder::new().with_ice_servers(servidores).with_certificates(vec![certificado]).build();
    let pc = PeerConnectionBuilder::new()
        .with_configuration(config)
        // Paciencia: con los códigos a mano el otro lado tarda minutos en pegar la respuesta; con los 25 s de serie la
        // conexión se daba por fallida antes de que el navegador empezara (fase 0)
        .with_setting_engine(
            SettingEngineBuilder::new()
                .with_ice_timeouts(Some(Duration::from_secs(300)), Some(Duration::from_secs(300)), Some(Duration::from_secs(2)))
                .build(),
        )
        .with_handler(Arc::new(Manejador { runtime: runtime.clone(), eventos: eventos.clone(), srflx: srflx_tx, completo: completo_tx }))
        .with_runtime(runtime.clone())
        .with_udp_addrs(vec!["0.0.0.0:0".to_string()])
        .with_data_channel_send_buffer_limit(MAX_BUFER_CANAL)
        .build()
        .await
        .map_err(|e| format!("No se pudo preparar la conexión directa: {e}"))?;
    let oferta = RTCSessionDescription::offer(contenido.sdp).map_err(|_| "La oferta del jugador no es válida")?;
    pc.set_remote_description(oferta).await.map_err(|e| format!("La oferta del jugador no vale: {e}"))?;
    let respuesta = pc.create_answer(None).await.map_err(|e| format!("No se pudo contestar: {e}"))?;
    pc.set_local_description(respuesta).await.map_err(|e| format!("No se pudo contestar: {e}"))?;

    // Sin trickle: se manda todo de una vez. En cuanto hay un candidato público (+ una gracia), o a los 1,5 s
    tokio::select! {
        _ = completo.recv() => {}
        _ = async { srflx.recv().await; tokio::time::sleep(GRACIA_SRFLX).await } => {}
        _ = tokio::time::sleep(TOPE_RESPUESTA) => {}
    }
    let local = pc.local_description().await.ok_or("No se pudo contestar")?;

    let nonce = rand::random::<[u8; 12]>();
    let respuesta = Contenido {
        tipo: "respuesta".into(),
        sdp: local.sdp,
        t: codigo::ahora_ms(),
        n: nonce.iter().map(|b| format!("{b:02x}")).collect(),
    };
    let cifrado = codigo::cifrar(&secreto, &respuesta, rand::random::<[u8; 12]>());

    tokio::spawn(vida_del_peer(red.clone(), pc, recibidos, eventos, plaza));
    Ok(cifrado)
}

/// Lo que dura la conexión: espera a los tres canales, arma el transporte, lanza `atender` y reparte lo que llega.
async fn vida_del_peer(
    red: Arc<Red>,
    pc: impl PeerConnection,
    mut recibidos: mpsc::UnboundedReceiver<Ev>,
    eventos: mpsc::UnboundedSender<Ev>,
    plaza: PlazaPeer,
) {
    let mut canales: HashMap<String, Arc<dyn DataChannel>> = HashMap::new();
    let mut plaza = Some(plaza);
    let mut entrada: Option<mpsc::Sender<Trama>> = None;
    let mut recomponer = Recomponer::nuevo(MAX_MENSAJE);
    let limite = tokio::time::sleep(TIEMPO_ABRIRSE);
    tokio::pin!(limite);

    loop {
        let ev = tokio::select! {
            ev = recibidos.recv() => ev,
            _ = &mut limite, if entrada.is_none() => None,
        };
        let Some(ev) = ev else { break };
        match ev {
            Ev::Fin => break,
            Ev::Abierto(etiqueta, canal) => {
                if CANALES.contains(&etiqueta.as_str()) {
                    canales.insert(etiqueta, canal);
                }
                if entrada.is_none() && CANALES.iter().all(|c| canales.contains_key(*c)) {
                    let (tx_salida, rx_salida) = mpsc::channel::<Trama>(CAPACIDAD);
                    let (tx_entrada, rx_entrada) = mpsc::channel::<Trama>(CAPACIDAD);
                    entrada = Some(tx_entrada);
                    plaza = None; // ya no está «a medias»
                    tokio::spawn(escribir(
                        rx_salida,
                        canales["juego"].clone(),
                        canales["imagenes"].clone(),
                        canales["arrastre"].clone(),
                        eventos.clone(),
                    ));
                    tokio::spawn(servidor::atender(red.clone(), Transporte { tx: tx_salida, rx: rx_entrada }));
                }
            }
            Ev::Mensaje(etiqueta, es_texto, datos) => {
                let Some(tx) = entrada.as_ref() else { continue };
                let trama = match etiqueta.as_str() {
                    "juego" => match recomponer.trozo(&datos) {
                        Ok(Some(texto)) => Trama::Texto(texto),
                        Ok(None) => continue,
                        Err(()) => break, // un trozo que no vale: se corta
                    },
                    "imagenes" => Trama::Binario(datos),
                    "arrastre" if es_texto => Trama::Efimero(String::from_utf8_lossy(&datos).into_owned()),
                    _ => continue,
                };
                // Cola de entrada llena (el host va lento): se espera, que `atender` la vacía
                if tx.send(trama).await.is_err() {
                    break; // `atender` terminó
                }
            }
        }
    }
    drop(plaza);
    drop(entrada);
    let _ = pc.close().await;
}

/// Saca lo que `atender` manda al jugador por el canal que toca. Al terminar `atender` (se suelta su `tx`), cierra.
async fn escribir(
    mut salida: mpsc::Receiver<Trama>,
    juego: Arc<dyn DataChannel>,
    imagenes: Arc<dyn DataChannel>,
    arrastre: Arc<dyn DataChannel>,
    eventos: mpsc::UnboundedSender<Ev>,
) {
    while let Some(trama) = salida.recv().await {
        let ok = match trama {
            Trama::Texto(s) => {
                let mut ok = true;
                for trozo in trocear(&s) {
                    // `send` espera si el búfer del canal está lleno: es la contrapresión hacia `atender`
                    if juego.send(BytesMut::from(&trozo[..])).await.is_err() {
                        ok = false;
                        break;
                    }
                }
                ok
            }
            Trama::Binario(b) => imagenes.send(BytesMut::from(&b[..])).await.is_ok(),
            // Puede perderse: si el búfer está lleno o falla, no importa (el siguiente lo sustituye)
            Trama::Efimero(s) => {
                let _ = arrastre.try_send_text(&s).await;
                true
            }
        };
        if !ok {
            break;
        }
    }
    let _ = eventos.send(Ev::Fin);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use crate::red::imagenes;
    use crate::red::protocolo::{MensajeCliente, MensajeHost, VERSION_PROTOCOLO};
    use crate::tablero_general::almacen::pruebas::dir_temporal;
    use crate::tablero_general::almacen::Almacen;
    use crate::tablero_general::estado::EstadoPartida;
    use crate::tablero_general::modelos::Mapa;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use webrtc::data_channel::RTCDataChannelInit;

    // ---- Un navegador de mentira: el mismo `webrtc-rs` haciendo de jugador ----

    struct ManejadorCliente {
        reunidos: mpsc::Sender<()>,
    }

    #[async_trait::async_trait]
    impl PeerConnectionEventHandler for ManejadorCliente {
        async fn on_ice_gathering_state_change(&self, estado: RTCIceGatheringState) {
            if estado == RTCIceGatheringState::Complete {
                let _ = self.reunidos.try_send(());
            }
        }
    }

    type Evento = (&'static str, bool, Vec<u8>);

    fn leer_canal(rt: &Arc<dyn Runtime>, canal: Arc<dyn DataChannel>, etiqueta: &'static str, tx: mpsc::UnboundedSender<Evento>) {
        rt.spawn(Box::pin(async move {
            while let Some(ev) = canal.poll().await {
                if let DataChannelEvent::OnMessage(m) = ev {
                    let _ = tx.send((etiqueta, m.is_string, m.data.to_vec()));
                }
            }
        }));
    }

    /// Lo que le llega al jugador: mensajes del protocolo por `juego` y trozos de imagen por `imagenes`.
    struct Entrada {
        eventos: mpsc::UnboundedReceiver<Evento>,
        recomponer: Recomponer,
        imagenes: Vec<Vec<u8>>,
    }

    impl Entrada {
        async fn mensaje(&mut self) -> MensajeHost {
            loop {
                let (canal, _, datos) = tokio::time::timeout(Duration::from_secs(15), self.eventos.recv()).await.expect("no llegó a tiempo").expect("cerrado");
                match canal {
                    "juego" => {
                        if let Some(texto) = self.recomponer.trozo(&datos).unwrap() {
                            return serde_json::from_str(&texto).unwrap();
                        }
                    }
                    "imagenes" => self.imagenes.push(datos),
                    _ => {}
                }
            }
        }
    }

    async fn decir(canal: &Arc<dyn DataChannel>, m: &MensajeCliente) {
        let json = serde_json::to_string(m).unwrap();
        for trozo in trocear(&json) {
            canal.send(BytesMut::from(&trozo[..])).await.unwrap();
        }
    }

    fn red_de_prueba(nombre: &str) -> Arc<Red> {
        let dir = dir_temporal(nombre);
        let almacen = Arc::new(Almacen::abrir(&dir).unwrap());
        Arc::new(Red::nueva(Arc::new(Mutex::new(EstadoPartida::default())), almacen, dir))
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn un_jugador_conecta_directo_con_codigos_y_juega_por_los_tres_canales() {
        let red = red_de_prueba("webrtc_bucle");
        red.abrir_local().await;
        let sala = red.sala_para_tests();
        let huella = sala.huella().expect("peer to peer: la sala tiene certificado");
        // Un fondo compartido para pedirlo por `imagenes`
        let dir = red.dir_sala().unwrap();
        std::fs::create_dir_all(dir.join("imagenes")).unwrap();
        let fondo: Vec<u8> = (0..40_000).map(|i| (i * 7 % 251) as u8).collect();
        std::fs::write(dir.join("imagenes/fondo.png"), &fondo).unwrap();
        red.estado_partida().mapas = vec![Mapa {
            id: 1, id_zona: 1, nombre: "M".into(), url_imagen_fondo: Some("imagenes/fondo.png".into()), ancho: 1000, alto: 1000,
            rejilla: 50, color_opacidad: vec![255, 255, 255, 76], grosor: 1, orden: 0, compartido: true,
        }];

        // --- El navegador: tres canales y una oferta, cifrada con el secreto del enlace ---
        let rt = default_runtime().unwrap();
        let (reunidos, mut al_reunir) = mpsc::channel(1);
        let pc = PeerConnectionBuilder::new()
            .with_configuration(RTCConfigurationBuilder::new().build())
            .with_handler(Arc::new(ManejadorCliente { reunidos }))
            .with_runtime(rt.clone())
            .with_udp_addrs(vec!["0.0.0.0:0".to_string()])
            .build()
            .await
            .unwrap();
        let juego = pc.create_data_channel("juego", None).await.unwrap();
        let imagenes_canal = pc.create_data_channel("imagenes", None).await.unwrap();
        let sin_orden = RTCDataChannelInit { ordered: false, max_retransmits: Some(0), ..Default::default() };
        let arrastre = pc.create_data_channel("arrastre", Some(sin_orden)).await.unwrap();
        let (tx, eventos) = mpsc::unbounded_channel();
        leer_canal(&rt, juego.clone(), "juego", tx.clone());
        leer_canal(&rt, imagenes_canal, "imagenes", tx.clone());
        leer_canal(&rt, arrastre.clone(), "arrastre", tx);
        let mut entrada = Entrada { eventos, recomponer: Recomponer::nuevo(MAX_MENSAJE), imagenes: Vec::new() };
        let oferta = pc.create_offer(None).await.unwrap();
        pc.set_local_description(oferta).await.unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(3), al_reunir.recv()).await;
        let sdp = pc.local_description().await.unwrap().sdp;
        let oferta = Contenido { tipo: "oferta".into(), sdp, t: codigo::ahora_ms(), n: "una-vez".into() };
        let codigo_oferta = codigo::cifrar(&sala.secreto, &oferta, [3; 12]);

        // --- El host contesta; los códigos malos se rechazan ---
        assert!(responder_con(&red, "no es un código", false).await.is_err());
        let otra_sala = codigo::cifrar("otro-secreto-de-otra-sala", &oferta, [4; 12]);
        assert!(responder_con(&red, &otra_sala, false).await.unwrap_err().contains("no es de esta sala"));
        let vieja = Contenido { t: codigo::ahora_ms() - 11 * 60 * 1000, n: "vieja".into(), ..oferta.clone() };
        assert!(responder_con(&red, &codigo::cifrar(&sala.secreto, &vieja, [5; 12]), false).await.unwrap_err().contains("viejo"));
        let respuesta_en_vez_de_oferta = Contenido { tipo: "respuesta".into(), n: "tipo".into(), ..oferta.clone() };
        assert!(responder_con(&red, &codigo::cifrar(&sala.secreto, &respuesta_en_vez_de_oferta, [6; 12]), false).await.is_err());

        let respuesta = responder_con(&red, &codigo_oferta, false).await.expect("el host contesta");
        assert!(responder_con(&red, &codigo_oferta, false).await.unwrap_err().contains("ya se usó"), "un código vale una vez");

        // La respuesta lleva la huella del certificado de la sala (la del enlace)
        let r = codigo::descifrar(&sala.secreto, &respuesta).unwrap();
        assert_eq!(r.tipo, "respuesta");
        let hex = hex::encode(URL_SAFE_NO_PAD.decode(&huella).unwrap()).to_uppercase();
        let con_puntos = hex.as_bytes().chunks(2).map(|p| std::str::from_utf8(p).unwrap()).collect::<Vec<_>>().join(":");
        assert!(r.sdp.contains(&format!("a=fingerprint:sha-256 {con_puntos}")), "falta la huella de la sala");
        pc.set_remote_description(RTCSessionDescription::answer(r.sdp).unwrap()).await.unwrap();

        // --- Por `juego`: reto, saludo con el enlace, nombre, bienvenida y vista (lo mismo que por el túnel) ---
        let MensajeHost::Reto { .. } = entrada.mensaje().await else { panic!("primero va el reto") };
        decir(&juego, &MensajeCliente::Hola { version: VERSION_PROTOCOLO, sala: sala.id.clone(), secreto: sala.secreto.clone(), prueba: None }).await;
        assert!(matches!(entrada.mensaje().await, MensajeHost::Identificate { .. }));
        decir(&juego, &MensajeCliente::Unirse { nombre: "Ana".into(), contrasena: None }).await;
        assert!(matches!(entrada.mensaje().await, MensajeHost::Bienvenido { .. }));
        assert!(matches!(entrada.mensaje().await, MensajeHost::Vista { .. }));

        // --- Una imagen: se pide por el canal SIN reintentos (el host lo trata igual) y llega por `imagenes` ---
        let pedir = MensajeCliente::PedirImagen { id: 1, ruta: "imagenes/fondo.png".into(), mini: None };
        arrastre.send_text(&serde_json::to_string(&pedir).unwrap()).await.unwrap();
        let MensajeHost::ImagenCabecera { id: 1, tam, .. } = entrada.mensaje().await else { panic!("sin cabecera de imagen") };
        assert_eq!(tam, fondo.len() as u64);
        let mut recibida = Vec::new();
        let t0 = std::time::Instant::now();
        while (recibida.len() as u64) < tam {
            assert!(t0.elapsed() < Duration::from_secs(15), "la imagen no llegó entera");
            while let Some(t) = (!entrada.imagenes.is_empty()).then(|| entrada.imagenes.remove(0)) {
                let (_, id, datos) = imagenes::leer_trozo(&t).unwrap();
                assert_eq!(id, 1);
                assert!(t.len() <= imagenes::TROZO + 5);
                recibida.extend_from_slice(datos);
            }
            if (recibida.len() as u64) < tam {
                // Espera más trozos (`mensaje` los va guardando mientras espera otro mensaje)
                let (canal, _, datos) = tokio::time::timeout(Duration::from_secs(15), entrada.eventos.recv()).await.unwrap().unwrap();
                if canal == "imagenes" {
                    entrada.imagenes.push(datos);
                }
            }
        }
        assert_eq!(recibida, fondo, "la imagen llega entera y en orden por el canal de imágenes");

        // --- Un mensaje largo pasa por trozos en los dos sentidos: la vista, o un texto grande de ida ---
        decir(&juego, &MensajeCliente::SubirImagen { id: 2, ext: "x".repeat(40_000), tam: crate::tablero_general::imagenes::MAX_BYTES_SUBIDA + 1 }).await;
        assert!(matches!(entrada.mensaje().await, MensajeHost::SubidaError { id: 2, .. }), "el host leyó el mensaje de 40 KB recompuesto");

        let _ = pc.close().await;
        red.cerrar();
    }

    #[tokio::test]
    async fn sin_sala_o_en_red_local_no_hay_conexion_directa() {
        let red = red_de_prueba("webrtc_sin_sala");
        assert!(responder_con(&red, "x", false).await.unwrap_err().contains("cerrada"));
        red.guardar_config(&crate::red::config::ConfigRed { respaldo: crate::red::config::Respaldo::Lan { puerto: 18999 } }).unwrap();
        red.abrir().await.unwrap();
        assert!(responder_con(&red, "x", false).await.unwrap_err().contains("Peer to peer"));
        red.cerrar();
    }
}
