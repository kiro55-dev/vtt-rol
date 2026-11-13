// src-tauri/src/red/senal.rs
//
// Señalización AUTOMÁTICA de la conexión directa por relés públicos de Nostr (fase 4). El jugador publica su oferta
// (el mismo código cifrado de `codigo.rs`) en unos relés; el host, que escucha en ellos, la contesta con
// `webrtc::responder_con` y publica la respuesta por el mismo camino. Los relés solo ven bultos opacos:
//
//  * El contenido es el código de `codigo.rs` (AES-256-GCM con una clave del secreto del enlace): ni el SDP ni las IPs
//    de los candidatos se ven, y sin el enlace no se fabrica una oferta que el host acepte.
//  * La etiqueta con la que se encuentran es un HMAC del secreto: quien no tenga el enlace ni sabe que la sala existe.
//  * Aunque un relé alterase mensajes, el navegador rechaza una respuesta cuyo certificado no sea el del enlace (`f`).
//
// Eventos efímeros (kind 27184, rango 20000–29999: los relés los reenvían y no los guardan) firmados con una clave
// Schnorr nueva por evento (los relés la exigen; no identifica a nadie). Espejo en `src/lib/red/senal.ts`.
//
// Esta es la parte del HOST: una tarea por relé (conecta, se suscribe, reconecta con espera creciente) y una que
// atiende las ofertas (con tope por minuto) y reparte las respuestas a todos los relés.
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use k256::schnorr::SigningKey;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async_tls_with_config, Connector};

use crate::red::codigo::{self, MAX_CODIGO};
use crate::red::webrtc;
use crate::red::Red;

/// El tipo de evento (efímero). No es el 24242 de Blossom.
pub const KIND: u32 = 27184;

/// Relés por los que se intercambian las ofertas (los que funcionaron al probarlos: docs/red.md, «Relés de Nostr»). Hay que
/// cambiarlos a la vez aquí y en `src/lib/red/senal.ts`, como la versión del protocolo.
pub const RELES: [&str; 4] = ["wss://relay.snort.social", "wss://relay.primal.net", "wss://nostr.oxtr.dev", "wss://offchain.pub"];

/// Un evento no puede ocupar más (lo que llega de un relé y lo que se publica).
const MAX_EVENTO: usize = 32 * 1024;
/// El `created_at` de una oferta no puede alejarse más de esto de la hora del host.
const MARGEN_SEGUNDOS: u64 = 120;
/// Ofertas que el host atiende por minuto, de cualquier relé (el resto se tira sin gastar nada).
const OFERTAS_POR_MINUTO: usize = 6;
/// Eventos ya vistos que se recuerdan (el mismo evento llega por varios relés).
const VISTOS: usize = 256;
const TIEMPO_CONECTAR: Duration = Duration::from_secs(10);
const CADA_PING: Duration = Duration::from_secs(30);
/// Un relé que lleva tanto sin decir nada (ni un pong) se da por muerto y se reconecta.
const SILENCIO_MAX: Duration = Duration::from_secs(100);
const ESPERA_MIN: Duration = Duration::from_secs(1);
const ESPERA_MAX: Duration = Duration::from_secs(60);

// ---------------------------------------------------------------------------
// Eventos (puro)
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Evento {
    pub id: String,
    pub pubkey: String,
    pub created_at: u64,
    pub kind: u32,
    pub tags: Vec<Vec<String>>,
    pub content: String,
    pub sig: String,
}

/// La etiqueta de las ofertas (`"oferta"`) o de las respuestas (`"respuesta"`) de esta sala: los 16 primeros bytes
/// (en hex) del HMAC-SHA256 del secreto del enlace. Solo la conoce quien tiene el enlace.
pub fn etiqueta(secreto: &str, tipo: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secreto.as_bytes()).expect("HMAC acepta cualquier clave");
    mac.update(format!("openroll-senal-{tipo}").as_bytes());
    hex::encode(&mac.finalize().into_bytes()[..16])
}

/// El `id` de NIP-01: SHA-256 (en hex) del JSON `[0, pubkey, created_at, kind, tags, content]`.
pub fn calcular_id(pubkey: &str, created_at: u64, kind: u32, tags: &[Vec<String>], content: &str) -> String {
    hex::encode(digest_id(pubkey, created_at, kind, tags, content))
}

fn digest_id(pubkey: &str, created_at: u64, kind: u32, tags: &[Vec<String>], content: &str) -> [u8; 32] {
    let json = serde_json::to_string(&json!([0, pubkey, created_at, kind, tags, content])).expect("se puede serializar");
    Sha256::digest(json.as_bytes()).into()
}

/// Crea y firma un evento con `clave` (32 bytes, una escalar válida) y la aleatoriedad auxiliar `aux`.
pub fn crear(clave: &[u8; 32], aux: &[u8; 32], created_at: u64, tags: Vec<Vec<String>>, content: String) -> Result<Evento, String> {
    let sk = SigningKey::from_bytes(clave).map_err(|_| "clave no válida")?;
    let pubkey = hex::encode(sk.verifying_key().to_bytes());
    let id = digest_id(&pubkey, created_at, KIND, &tags, &content);
    let sig = sk.sign_raw(&id, aux).map_err(|_| "no se pudo firmar")?;
    Ok(Evento { id: hex::encode(id), pubkey, created_at, kind: KIND, tags, content, sig: hex::encode(sig.to_bytes()) })
}

/// Un evento con una clave aleatoria nueva (la que se usa de verdad).
pub fn crear_aleatorio(created_at: u64, tags: Vec<Vec<String>>, content: String) -> Result<Evento, String> {
    loop {
        let clave = rand::random::<[u8; 32]>();
        if SigningKey::from_bytes(&clave).is_ok() {
            return crear(&clave, &rand::random::<[u8; 32]>(), created_at, tags, content);
        }
    }
}

/// ¿El `id` es el del contenido?
pub fn id_valido(e: &Evento) -> bool {
    calcular_id(&e.pubkey, e.created_at, e.kind, &e.tags, &e.content) == e.id
}

/// ¿La firma Schnorr es válida para ese `id`? (El host no la exige: la seguridad es el cifrado; los tests la usan.)
#[cfg(test)]
pub fn firma_valida(e: &Evento) -> bool {
    let (Ok(pk), Ok(id), Ok(sig)) = (hex::decode(&e.pubkey), hex::decode(&e.id), hex::decode(&e.sig)) else { return false };
    let (Ok(vk), Ok(sig)) = (k256::schnorr::VerifyingKey::from_bytes(&pk), k256::schnorr::Signature::try_from(sig.as_slice())) else { return false };
    vk.verify_raw(&id, &sig).is_ok()
}

fn etiqueta_de<'a>(e: &'a Evento, nombre: &str) -> Option<&'a str> {
    e.tags.iter().find(|t| t.len() >= 2 && t[0] == nombre).map(|t| t[1].as_str())
}

/// Lo que se exige a una oferta antes de gastar nada en ella: forma, etiqueta de la sala, hora y tamaño.
fn oferta_valida(e: &Evento, etiqueta_oferta: &str, ahora_s: u64) -> bool {
    e.kind == KIND
        && etiqueta_de(e, "t") == Some(etiqueta_oferta)
        && e.created_at.abs_diff(ahora_s) <= MARGEN_SEGUNDOS
        && e.content.len() <= MAX_CODIGO
        && id_valido(e)
}

/// Lo que dice un mensaje de un relé, si es un evento: `["EVENT", <suscripción>, <evento>]`.
fn leer_evento(texto: &str) -> Option<Evento> {
    if texto.len() > MAX_EVENTO {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(texto).ok()?;
    let a = v.as_array()?;
    if a.first()?.as_str()? != "EVENT" || a.len() < 3 {
        return None;
    }
    serde_json::from_value(a[2].clone()).ok()
}

fn ahora_s() -> u64 {
    codigo::ahora_ms() / 1000
}

/// Un cubo sencillo: como mucho `max` por minuto.
struct Tope {
    marcas: VecDeque<Instant>,
    max: usize,
}

impl Tope {
    fn nuevo(max: usize) -> Self {
        Tope { marcas: VecDeque::new(), max }
    }

    fn permitir(&mut self) -> bool {
        let ahora = Instant::now();
        while self.marcas.front().is_some_and(|t| ahora.duration_since(*t) > Duration::from_secs(60)) {
            self.marcas.pop_front();
        }
        if self.marcas.len() >= self.max {
            return false;
        }
        self.marcas.push_back(ahora);
        true
    }
}

// ---------------------------------------------------------------------------
// Conexión con los relés
// ---------------------------------------------------------------------------

/// Una escucha de relés en marcha: se para al soltarla.
pub struct Senal {
    total: usize,
    conectados: Arc<AtomicUsize>,
    tareas: Vec<JoinHandle<()>>,
}

impl Senal {
    pub fn conectados(&self) -> usize {
        self.conectados.load(Ordering::Relaxed)
    }

    pub fn total(&self) -> usize {
        self.total
    }
}

impl Drop for Senal {
    fn drop(&mut self) {
        for t in &self.tareas {
            t.abort();
        }
    }
}

/// El TLS de los relés. En el proceso conviven dos proveedores de rustls (`aws-lc-rs`, de reqwest, y `ring`, de
/// webrtc): el «por defecto» no se puede averiguar y entraría en pánico, así que se nombra `ring` a mano.
fn conector() -> Result<Connector, String> {
    static CONFIG: OnceLock<Result<Arc<rustls::ClientConfig>, String>> = OnceLock::new();
    let c = CONFIG.get_or_init(|| {
        let raices = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_root_certificates(raices)
            .with_no_client_auth();
        Ok(Arc::new(config))
    });
    match c {
        Ok(c) => Ok(Connector::Rustls(c.clone())),
        Err(e) => Err(e.clone()),
    }
}

/// Lo que se suscribe el host: las ofertas de esta sala de los últimos 2 minutos en adelante.
fn peticion_ofertas(etiqueta_oferta: &str) -> String {
    json!(["REQ", "ofertas", { "kinds": [KIND], "#t": [etiqueta_oferta], "since": ahora_s().saturating_sub(MARGEN_SEGUNDOS) }]).to_string()
}

/// Una conexión con un relé, de principio a fin: se suscribe y reparte lo que llega; publica lo que le dicen.
async fn sesion(
    url: &str,
    etiqueta_oferta: &str,
    entrantes: &mpsc::Sender<Evento>,
    salida: &mut broadcast::Receiver<Arc<Evento>>,
    conectados: &AtomicUsize,
) -> Result<(), String> {
    let config = WebSocketConfig::default().max_message_size(Some(MAX_EVENTO * 2)).max_frame_size(Some(MAX_EVENTO * 2));
    let (mut ws, _) = tokio::time::timeout(
        TIEMPO_CONECTAR,
        connect_async_tls_with_config(url, Some(config), false, Some(conector()?)),
    )
    .await
    .map_err(|_| "no conecta a tiempo".to_string())?
    .map_err(|e| e.to_string())?;
    ws.send(Message::Text(peticion_ofertas(etiqueta_oferta).into())).await.map_err(|e| e.to_string())?;
    conectados.fetch_add(1, Ordering::Relaxed);
    let resultado = async {
        let mut ping = tokio::time::interval(CADA_PING);
        ping.tick().await;
        let mut ultimo = Instant::now();
        loop {
            tokio::select! {
                m = ws.next() => {
                    let Some(m) = m else { return Ok(()) };
                    ultimo = Instant::now();
                    match m.map_err(|e| e.to_string())? {
                        Message::Text(t) => {
                            if let Some(e) = leer_evento(t.as_str()) {
                                // Si el host va lento se tira: otra oferta llegará (o el jugador pasará al respaldo)
                                let _ = entrantes.try_send(e);
                            }
                        }
                        Message::Close(_) => return Ok(()),
                        _ => {}
                    }
                }
                e = salida.recv() => match e {
                    Ok(e) => {
                        let texto = json!(["EVENT", &*e]).to_string();
                        ws.send(Message::Text(texto.into())).await.map_err(|e| e.to_string())?;
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return Ok(()),
                },
                _ = ping.tick() => {
                    if ultimo.elapsed() > SILENCIO_MAX {
                        return Err("el relé no contesta".into());
                    }
                    ws.send(Message::Ping(Vec::new().into())).await.map_err(|e| e.to_string())?;
                }
            }
        }
    }
    .await;
    conectados.fetch_sub(1, Ordering::Relaxed);
    resultado
}

/// Mantiene la conexión con un relé: si se cae o no conecta, reintenta con espera creciente (1 s → 60 s).
async fn mantener_rele(
    url: String,
    etiqueta_oferta: String,
    entrantes: mpsc::Sender<Evento>,
    mut salida: broadcast::Receiver<Arc<Evento>>,
    conectados: Arc<AtomicUsize>,
) {
    let mut espera = ESPERA_MIN;
    loop {
        let desde = Instant::now();
        let _ = sesion(&url, &etiqueta_oferta, &entrantes, &mut salida, &conectados).await;
        // Si estuvo un buen rato conectado, se vuelve a empezar por la espera corta
        if desde.elapsed() > Duration::from_secs(30) {
            espera = ESPERA_MIN;
        }
        tokio::time::sleep(espera).await;
        espera = (espera * 2).min(ESPERA_MAX);
    }
}

/// Atiende las ofertas que llegan de los relés: las filtra, las contesta y reparte la respuesta a todos.
async fn atender_ofertas(
    red: Arc<Red>,
    secreto: String,
    mut entrantes: mpsc::Receiver<Evento>,
    salida: broadcast::Sender<Arc<Evento>>,
    stun: bool,
) {
    let etiqueta_oferta = etiqueta(&secreto, "oferta");
    let etiqueta_respuesta = etiqueta(&secreto, "respuesta");
    let mut vistos: VecDeque<String> = VecDeque::new();
    let mut tope = Tope::nuevo(OFERTAS_POR_MINUTO);
    while let Some(e) = entrantes.recv().await {
        // Lo barato primero: el mismo evento llega por varios relés
        if vistos.contains(&e.id) {
            continue;
        }
        vistos.push_back(e.id.clone());
        while vistos.len() > VISTOS {
            vistos.pop_front();
        }
        if !oferta_valida(&e, &etiqueta_oferta, ahora_s()) || !tope.permitir() {
            continue;
        }
        let Ok(oferta) = codigo::descifrar(&secreto, &e.content) else { continue };
        let red = red.clone();
        let salida = salida.clone();
        let etiqueta_respuesta = etiqueta_respuesta.clone();
        tokio::spawn(async move {
            let Ok(respuesta) = webrtc::responder_con(&red, &e.content, stun).await else { return };
            let tags = vec![vec!["t".to_string(), etiqueta_respuesta], vec!["x".to_string(), oferta.n]];
            if let Ok(ev) = crear_aleatorio(ahora_s(), tags, respuesta) {
                let _ = salida.send(Arc::new(ev));
            }
        });
    }
}

/// Empieza a escuchar en `reles` las ofertas de la sala abierta (en peer to peer). Hay que llamarla dentro de un
/// runtime de tokio. `stun = false` solo en los tests.
pub fn arrancar(red: &Arc<Red>, reles: &[String], stun: bool) -> Result<Senal, String> {
    let (secreto, _) = red.datos_directo()?;
    let etiqueta_oferta = etiqueta(&secreto, "oferta");
    let (tx_entrantes, rx_entrantes) = mpsc::channel::<Evento>(64);
    let (salida, _) = broadcast::channel::<Arc<Evento>>(16);
    let conectados = Arc::new(AtomicUsize::new(0));
    let mut tareas = Vec::new();
    for url in reles {
        tareas.push(tokio::spawn(mantener_rele(url.clone(), etiqueta_oferta.clone(), tx_entrantes.clone(), salida.subscribe(), conectados.clone())));
    }
    tareas.push(tokio::spawn(atender_ofertas(red.clone(), secreto, rx_entrantes, salida, stun)));
    Ok(Senal { total: reles.len(), conectados, tareas })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAVE: [u8; 32] = [1; 32];
    const AUX: [u8; 32] = [0; 32];
    const SECRETO: &str = "0123456789abcdef_-XYZ";

    fn tags() -> Vec<Vec<String>> {
        vec![vec!["t".into(), "etiqueta".into()]]
    }

    #[test]
    fn un_evento_creado_es_valido_y_se_rompe_si_se_toca() {
        let e = crear(&CLAVE, &AUX, 1_700_000_000, tags(), "hola".into()).unwrap();
        assert_eq!(e.kind, KIND);
        assert!(id_valido(&e));
        assert!(firma_valida(&e));
        let mut otro = e.clone();
        otro.content = "adiós".into();
        assert!(!id_valido(&otro), "el id no cuadra con otro contenido");
        let mut otro = e.clone();
        otro.sig = "0".repeat(128);
        assert!(!firma_valida(&otro));
        // La firma se hace con aleatoriedad auxiliar: dos eventos con claves nuevas son distintos
        let a = crear_aleatorio(1, tags(), "x".into()).unwrap();
        let b = crear_aleatorio(1, tags(), "x".into()).unwrap();
        assert_ne!(a.pubkey, b.pubkey);
        assert!(firma_valida(&a) && firma_valida(&b));
    }

    #[test]
    fn las_etiquetas_dependen_del_secreto_y_del_tipo() {
        let o = etiqueta(SECRETO, "oferta");
        let r = etiqueta(SECRETO, "respuesta");
        assert_eq!(o.len(), 32);
        assert_ne!(o, r);
        assert_ne!(o, etiqueta("otro-secreto-de-otra-sala", "oferta"));
        assert_eq!(o, etiqueta(SECRETO, "oferta"), "determinista");
    }

    #[test]
    fn solo_se_lee_un_evento_de_un_mensaje_de_relé() {
        let e = crear(&CLAVE, &AUX, 1, tags(), "c".into()).unwrap();
        let ok = json!(["EVENT", "ofertas", e]).to_string();
        assert_eq!(leer_evento(&ok), Some(e.clone()));
        assert_eq!(leer_evento(&json!(["OK", e.id, true, ""]).to_string()), None);
        assert_eq!(leer_evento(&json!(["NOTICE", "hola"]).to_string()), None);
        assert_eq!(leer_evento("no es json"), None);
        assert_eq!(leer_evento(&json!(["EVENT", "x", {"id": 3}]).to_string()), None);
        let enorme = json!(["EVENT", "ofertas", {"content": "x".repeat(MAX_EVENTO)}]).to_string();
        assert_eq!(leer_evento(&enorme), None, "un mensaje demasiado grande se tira");
    }

    #[test]
    fn una_oferta_se_filtra_por_etiqueta_hora_y_tamano() {
        let t = etiqueta(SECRETO, "oferta");
        let ahora = 1_700_000_000;
        let hecha = |tags: Vec<Vec<String>>, creada: u64, c: String| crear(&CLAVE, &AUX, creada, tags, c).unwrap();
        let buena = hecha(vec![vec!["t".into(), t.clone()]], ahora, "c".into());
        assert!(oferta_valida(&buena, &t, ahora));
        assert!(oferta_valida(&buena, &t, ahora + MARGEN_SEGUNDOS));
        assert!(!oferta_valida(&buena, &t, ahora + MARGEN_SEGUNDOS + 1), "vieja");
        assert!(!oferta_valida(&buena, &t, ahora - MARGEN_SEGUNDOS - 1), "del futuro");
        assert!(!oferta_valida(&hecha(vec![vec!["t".into(), "otra".into()]], ahora, "c".into()), &t, ahora), "otra sala");
        assert!(!oferta_valida(&hecha(vec![], ahora, "c".into()), &t, ahora), "sin etiqueta");
        assert!(!oferta_valida(&hecha(vec![vec!["t".into(), t.clone()]], ahora, "x".repeat(MAX_CODIGO + 1)), &t, ahora), "demasiado grande");
        let mut mala = buena.clone();
        mala.content = "otro".into();
        assert!(!oferta_valida(&mala, &t, ahora), "id que no cuadra");
    }

    #[test]
    fn el_tope_por_minuto_corta_el_exceso() {
        let mut t = Tope::nuevo(3);
        assert!(t.permitir() && t.permitir() && t.permitir());
        assert!(!t.permitir());
    }

    #[test]
    fn el_tls_de_los_reles_se_construye_sin_proveedor_por_defecto() {
        assert!(conector().is_ok());
    }

    /// Hecho con `@noble/curves` (src/lib/red/senal.ts): los dos lados calculan lo mismo y se verifican entre sí.
    #[test]
    fn vector_compartido_con_el_navegador() {
        let de_ts: Evento = serde_json::from_str(
            r#"{"id":"d814104af9a6ac42f3f094ba78b7af7a3b1763fafc88803d55387ec5dc310202","pubkey":"1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f","created_at":1700000000,"kind":27184,"tags":[["t","etiqueta"]],"content":"hola","sig":"4f87b7ebaa9511c4b5c1204a154086ba752e0b4b835d8550898bec1e03373a505b4d73e302c6507cc0828d037ac2ee7de5d36bc3d86d56d44c21b5c7ad5eee86"}"#,
        )
        .unwrap();
        assert!(id_valido(&de_ts) && firma_valida(&de_ts), "Rust acepta el evento que hizo el navegador");
        assert_eq!(crear(&CLAVE, &AUX, 1_700_000_000, tags(), "hola".into()).unwrap(), de_ts, "y hace el mismo, firma incluida");
        assert_eq!(etiqueta(SECRETO, "oferta"), "3791f4b8ce20a3e2136792ead50c504b");
        assert_eq!(etiqueta(SECRETO, "respuesta"), "a25020d1b759722a234a48235b24f009");
    }

    // ---- Un relé de mentira (axum) y un navegador de mentira (webrtc-rs) ----

    use axum::extract::ws::{Message as Msg, WebSocket, WebSocketUpgrade};
    use axum::routing::get;
    use axum::Router;
    use std::sync::Mutex;
    use ::webrtc::peer_connection::{PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCConfigurationBuilder, RTCIceGatheringState};
    use ::webrtc::runtime::default_runtime;
    use base64::Engine;
    use crate::red::codigo::Contenido;

    /// Un relé mínimo: guarda las suscripciones (`#t` y `#x`) de cada conexión y reparte los eventos a quien encajan.
    async fn rele_falso(puerto: u16) -> JoinHandle<()> {
        let (tx, _) = broadcast::channel::<String>(64);
        let app = Router::new().route(
            "/",
            get(move |ws: WebSocketUpgrade| {
                let tx = tx.clone();
                async move { ws.on_upgrade(move |s| conexion_falsa(s, tx)) }
            }),
        );
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", puerto)).await.unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        })
    }

    async fn conexion_falsa(mut socket: WebSocket, tx: broadcast::Sender<String>) {
        let mut rx = tx.subscribe();
        let mut subs: Vec<(String, serde_json::Value)> = Vec::new();
        loop {
            tokio::select! {
                m = socket.recv() => {
                    let Some(Ok(Msg::Text(t))) = m else { return };
                    let Ok(v) = serde_json::from_str::<serde_json::Value>(t.as_str()) else { continue };
                    match v[0].as_str() {
                        Some("REQ") => subs.push((v[1].as_str().unwrap_or("").to_string(), v[2].clone())),
                        Some("EVENT") => {
                            let _ = tx.send(v[1].to_string());
                            let _ = socket.send(Msg::Text(json!(["OK", v[1]["id"], true, ""]).to_string().into())).await;
                        }
                        _ => {}
                    }
                }
                e = rx.recv() => {
                    let Ok(e) = e else { continue };
                    let ev: serde_json::Value = serde_json::from_str(&e).unwrap();
                    for (sub, f) in &subs {
                        let tag = |n: &str| ev["tags"].as_array().unwrap().iter().find(|t| t[0] == n).map(|t| t[1].clone());
                        let encaja = |clave: &str, n: &str| f[clave].as_array().is_none_or(|v| tag(n).is_some_and(|x| v.contains(&x)));
                        if encaja("#t", "t") && encaja("#x", "x") {
                            let _ = socket.send(Msg::Text(json!(["EVENT", sub, ev]).to_string().into())).await;
                        }
                    }
                }
            }
        }
    }

    struct Manejador(mpsc::Sender<()>);

    #[async_trait::async_trait]
    impl PeerConnectionEventHandler for Manejador {
        async fn on_ice_gathering_state_change(&self, estado: RTCIceGatheringState) {
            if estado == RTCIceGatheringState::Complete {
                let _ = self.0.try_send(());
            }
        }
    }

    /// La oferta (SDP) de un navegador de mentira con los tres canales.
    async fn oferta_de_navegador() -> (impl PeerConnection, String) {
        let rt = default_runtime().unwrap();
        let (reunidos, mut al_reunir) = mpsc::channel(1);
        let pc = PeerConnectionBuilder::new()
            .with_configuration(RTCConfigurationBuilder::new().build())
            .with_handler(Arc::new(Manejador(reunidos)))
            .with_runtime(rt)
            .with_udp_addrs(vec!["0.0.0.0:0".to_string()])
            .build()
            .await
            .unwrap();
        for c in ["juego", "imagenes", "arrastre"] {
            pc.create_data_channel(c, None).await.unwrap();
        }
        let oferta = pc.create_offer(None).await.unwrap();
        pc.set_local_description(oferta).await.unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(3), al_reunir.recv()).await;
        let sdp = pc.local_description().await.unwrap().sdp;
        (pc, sdp)
    }

    type Cliente = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

    async fn cliente(url: &str) -> Cliente {
        tokio_tungstenite::connect_async(url).await.unwrap().0
    }

    async fn esperar_reles(s: &Senal, n: usize) {
        for _ in 0..100 {
            if s.conectados() == n {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("relés conectados: {} (esperaba {n})", s.conectados());
    }

    /// El primer evento que llegue por `c` en `tope`, si llega.
    async fn siguiente_evento(c: &mut Cliente, tope: Duration) -> Option<Evento> {
        let t0 = Instant::now();
        while let Some(resto) = tope.checked_sub(t0.elapsed()) {
            let Ok(Some(Ok(Message::Text(t)))) = tokio::time::timeout(resto, c.next()).await else { continue };
            if let Some(e) = leer_evento(t.as_str()) {
                return Some(e);
            }
        }
        None
    }

    async fn publicar(web: &mut Cliente, e: Evento) {
        web.send(Message::Text(json!(["EVENT", e]).to_string().into())).await.unwrap();
    }

    fn red_de_prueba(nombre: &str) -> Arc<Red> {
        use crate::tablero_general::almacen::pruebas::dir_temporal;
        use crate::tablero_general::almacen::Almacen;
        use crate::tablero_general::estado::EstadoPartida;
        let dir = dir_temporal(nombre);
        let almacen = Arc::new(Almacen::abrir(&dir).unwrap());
        Arc::new(Red::nueva(Arc::new(Mutex::new(EstadoPartida::default())), almacen, dir))
    }

    /// Con los relés de verdad (hace falta internet): comprueba el TLS y que la escucha se conecta. A mano:
    /// `cargo test relés_reales -- --ignored`.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn reles_reales_aceptan_la_escucha_del_host() {
        let red = red_de_prueba("senal_reales");
        red.abrir_local().await;
        let reles: Vec<String> = RELES.iter().map(|r| r.to_string()).collect();
        let senal = arrancar(&red, &reles, false).unwrap();
        for _ in 0..150 {
            if senal.conectados() >= 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        println!("relés conectados: {} de {}", senal.conectados(), senal.total());
        assert!(senal.conectados() >= 1, "ningún relé real conectó (¿sin internet, o TLS roto?)");
        red.cerrar();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn una_oferta_por_el_rele_se_contesta_por_el_rele_y_lo_malo_se_ignora() {
        let red = red_de_prueba("senal_bucle");
        red.abrir_local().await;
        let sala = red.sala_para_tests();
        let huella = sala.huella().expect("peer to peer: hay certificado");

        // El relé arranca DESPUÉS de la escucha: se reconecta solo (el relé estaba caído)
        let puerto = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let url = format!("ws://127.0.0.1:{puerto}/");
        let senal = arrancar(&red, std::slice::from_ref(&url), false).unwrap();
        assert_eq!((senal.conectados(), senal.total()), (0, 1));
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(senal.conectados(), 0, "sin relé no hay conexión (y no se cae nada)");
        let _rele = rele_falso(puerto).await;
        esperar_reles(&senal, 1).await;

        // --- El navegador: una oferta cifrada, publicada por el relé ---
        let (pc, sdp) = oferta_de_navegador().await;
        let n = "oferta-uno".to_string();
        let contenido = Contenido { tipo: "oferta".into(), sdp, t: codigo::ahora_ms(), n: n.clone() };
        let codigo_oferta = codigo::cifrar(&sala.secreto, &contenido, [9; 12]);
        let mut web = cliente(&url).await;
        let filtro = json!(["REQ", "r", { "kinds": [KIND], "#t": [etiqueta(&sala.secreto, "respuesta")], "#x": [n] }]);
        web.send(Message::Text(filtro.to_string().into())).await.unwrap();
        let etiqueta_oferta = etiqueta(&sala.secreto, "oferta");

        // Lo malo primero, y no recibe respuesta: otra sala, vieja y de otro tipo de etiqueta
        let otra = crear_aleatorio(ahora_s(), vec![vec!["t".into(), etiqueta("otro-secreto", "oferta")]], codigo_oferta.clone()).unwrap();
        publicar(&mut web, otra).await;
        let vieja = crear_aleatorio(ahora_s() - 3600, vec![vec!["t".into(), etiqueta_oferta.clone()]], codigo_oferta.clone()).unwrap();
        publicar(&mut web, vieja).await;
        let basura = crear_aleatorio(ahora_s(), vec![vec!["t".into(), etiqueta_oferta.clone()]], "esto no descifra".into()).unwrap();
        publicar(&mut web, basura).await;
        assert!(siguiente_evento(&mut web, Duration::from_millis(1200)).await.is_none(), "lo malo no se contesta");

        // La buena
        let buena = crear_aleatorio(ahora_s(), vec![vec!["t".into(), etiqueta_oferta.clone()]], codigo_oferta.clone()).unwrap();
        publicar(&mut web, buena).await;
        let respuesta = siguiente_evento(&mut web, Duration::from_secs(10)).await.expect("el host contesta por el relé");
        assert!(id_valido(&respuesta) && firma_valida(&respuesta));
        assert_eq!(etiqueta_de(&respuesta, "t"), Some(etiqueta(&sala.secreto, "respuesta").as_str()));
        assert_eq!(etiqueta_de(&respuesta, "x"), Some(n.as_str()), "va dirigida a esa oferta");
        let r = codigo::descifrar(&sala.secreto, &respuesta.content).unwrap();
        assert_eq!(r.tipo, "respuesta");
        // La respuesta lleva el certificado de la sala (el navegador lo comprueba contra el `f` del enlace)
        let hex = hex::encode(base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(&huella).unwrap()).to_uppercase();
        let con_puntos = hex.as_bytes().chunks(2).map(|p| std::str::from_utf8(p).unwrap()).collect::<Vec<_>>().join(":");
        assert!(r.sdp.contains(&format!("a=fingerprint:sha-256 {con_puntos}")));

        // La misma oferta otra vez (otro evento, mismo `n`): ya se usó, no hay segunda respuesta
        let repetida = crear_aleatorio(ahora_s(), vec![vec!["t".into(), etiqueta_oferta]], codigo_oferta).unwrap();
        publicar(&mut web, repetida).await;
        assert!(siguiente_evento(&mut web, Duration::from_millis(2000)).await.is_none(), "un código vale una vez");

        let _ = pc.close().await;
        drop(senal);
        red.cerrar();
    }
}
