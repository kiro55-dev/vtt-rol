// src-tauri/src/red/servidor.rs
//
// El servidor propio del host, en 127.0.0.1 (desde fuera solo se llega por el túnel):
//  * GET /ws       el WebSocket de cada jugador: saludo (ver protocolo.rs) y luego su vista y sus cambios.
//  * GET /imagen   fondos de mapa, fotos y miniaturas, solo si el jugador de esa sesión las ve
//                  (`visibilidad::recurso_visible_para`) o las ha subido él.
//  * POST /subir   la foto de una ficha suya, subida desde su navegador (solo imágenes de verdad, con tope).
//  Como el navegador está en otra dirección (GitHub Pages), /imagen y /subir llevan CORS para el origen de la
//  página (el mismo que acepta el WebSocket) y /subir contesta al OPTIONS.
//
// Seguridad del saludo:
//  * Solo se acepta el WebSocket desde el origen del cliente web (`Red::origenes`).
//  * Sin el `id` y el `secreto` del enlace no se pasa del primer mensaje.
//  * Contraseña: 5 fallos por conexión, 20 por minuto en total, y medio segundo de espera tras cada fallo.
//  * Mensajes de hasta 512 KiB (un .txt largo, una miniatura), tiempo límite para saludar y un tope de peticiones
//    por segundo (las acciones del jugador se hacen en orden, una a una).
use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

use crate::red::Red;
use crate::red::peticiones;
use crate::red::protocolo::{MensajeCliente, MensajeHost, VERSION_PROTOCOLO};
use crate::tablero_general::estado::DeltaPartida;
use crate::tablero_general::modelos::Jugador;
use crate::tablero_general::{archivos, jugadores, miniaturas, partida, visibilidad};

const MAX_MENSAJE: usize = 512 * 1024;
/// Peticiones por segundo de un jugador (arrastrar, tirar, editar...). Con más, se contestan con un error.
const PETICIONES_POR_SEGUNDO: u32 = 30;
const TIEMPO_SALUDO: Duration = Duration::from_secs(15);
/// Tiempo para escribir nombre y contraseña.
const TIEMPO_IDENTIFICARSE: Duration = Duration::from_secs(600);
/// Cloudflare corta un WebSocket sin tráfico a los 100 s.
const CADA_PING: Duration = Duration::from_secs(30);

/// Arranca el servidor en un puerto libre de 127.0.0.1. Devuelve el puerto y cómo apagarlo.
pub async fn arrancar(red: Arc<Red>) -> Result<(u16, oneshot::Sender<()>), String> {
    let escucha = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("No se pudo arrancar el servidor de la sala: {e}"))?;
    let puerto = escucha.local_addr().map_err(|e| e.to_string())?.port();
    let app = Router::new()
        .route("/", get(|| async { "OpenRoll" }))
        .route("/ws", get(ws))
.route("/imagen", get(imagen))
        .route("/subir", post(subir).layer(DefaultBodyLimit::max(crate::tablero_general::imagenes::MAX_BYTES_SUBIDA + 1024)))
        .layer(axum::middleware::from_fn_with_state(red.clone(), cors))
        .with_state(red);
    let (apagar, al_apagar) = oneshot::channel::<()>();
    tokio::spawn(async move {
        let servidor = axum::serve(escucha, app).with_graceful_shutdown(async {
            let _ = al_apagar.await;
        });
        if let Err(e) = servidor.await {
            eprintln!("[red] el servidor de la sala se ha parado: {e}");
        }
    });
    Ok((puerto, apagar))
}

// ------------------------------------------------------------------
// WebSocket
// ------------------------------------------------------------------

async fn ws(State(red): State<Arc<Red>>, cabeceras: HeaderMap, ws: WebSocketUpgrade) -> Response {
    let origen = cabeceras.get(header::ORIGIN).and_then(|o| o.to_str().ok()).unwrap_or("");
    if !red.origenes.iter().any(|o| o == origen) {
        return (StatusCode::FORBIDDEN, "Origen no permitido").into_response();
    }
    ws.max_message_size(MAX_MENSAJE).on_upgrade(move |socket| atender(red, socket))
}

type Emisor = SplitSink<WebSocket, Message>;
type Receptor = SplitStream<WebSocket>;

async fn enviar(tx: &mut Emisor, m: &MensajeHost) -> Result<(), ()> {
    let json = serde_json::to_string(m).map_err(|_| ())?;
    tx.send(Message::Text(json.into())).await.map_err(|_| ())
}

/// El siguiente mensaje del cliente (los pings se saltan). `None` si se cerró o mandó algo que no se entiende.
async fn recibir(rx: &mut Receptor) -> Option<MensajeCliente> {
    loop {
        match rx.next().await? {
            Ok(Message::Text(t)) => return serde_json::from_str(t.as_str()).ok(),
            Ok(Message::Ping(_) | Message::Pong(_)) => continue,
            _ => return None,
        }
    }
}

async fn rechazar(tx: &mut Emisor, motivo: &str) {
    let _ = enviar(tx, &MensajeHost::Rechazado { motivo: motivo.into() }).await;
    let _ = tx.close().await;
}

async fn atender(red: Arc<Red>, socket: WebSocket) {
    let (mut tx, mut rx) = socket.split();

    // 1. Reto y Hola: el enlace (y la prueba, si el navegador ya entró antes)
    let reto = crate::red::sala::reto();
    if enviar(&mut tx, &MensajeHost::Reto { reto: reto.clone() }).await.is_err() {
        return;
    }
    let Ok(Some(MensajeCliente::Hola { version, sala, secreto, prueba })) = timeout(TIEMPO_SALUDO, recibir(&mut rx)).await else {
        return rechazar(&mut tx, "Saludo no válido").await;
    };
    if version != VERSION_PROTOCOLO {
        return rechazar(&mut tx, "Versión distinta de la del host: recarga la página").await;
    }
    let (valido, recordado, pide_contrasena) = {
        let dentro = red.dentro();
        match dentro.as_ref() {
            Some(s) => (
                s.sala.coincide(&sala, &secreto),
                // La prueba se calcula con la dirección del túnel de ESTE host: una de otro no vale aquí
                match (prueba.as_deref(), s.tunel.as_deref()) {
                    (Some(p), Some(t)) => s.sala.jugador_de_prueba(&reto, t, p),
                    _ => None,
                },
                s.sala.pide_contrasena(),
            ),
            None => (false, None, false),
        }
    };
    if !valido {
        return rechazar(&mut tx, "Este enlace ya no vale: pide uno nuevo al host").await;
    }
    // La clave debe ser de un jugador que sigue en la partida
    let recordado = recordado.and_then(|id| red.estado_partida().jugadores.iter().find(|j| j.id == id).cloned());

    // 2. Si no se le recuerda: nombre y contraseña
    let (yo, token_nuevo) = match recordado {
        Some(j) => (j, None),
        None => match identificar(&red, &mut tx, &mut rx, pide_contrasena).await {
            Some(r) => r,
            None => return,
        },
    };

    // 3. Bienvenida y vista inicial. A partir de aquí, todo lo que se le envía pasa por su canal.
    let Some(sesion) = red.sesion_de(yo.id) else {
        return rechazar(&mut tx, "La sala se ha cerrado").await;
    };
    let bienvenida = MensajeHost::Bienvenido { yo: yo.clone(), token: token_nuevo, sesion: sesion.clone() };
    if enviar(&mut tx, &bienvenida).await.is_err() {
        return;
    }
    let (canal, mut salida) = mpsc::unbounded_channel();
    let respuestas = canal.clone();
    let avisos = canal.clone();
    let id_conexion = {
        let est = red.estado_partida(); // estado → dentro
        red.conectar(&est, yo.id, canal)
    };
    let Some(id_conexion) = id_conexion else {
        return rechazar(&mut tx, "La sala se ha cerrado").await;
    };
    red.avisar();

    // Las acciones del jugador se ejecutan en orden, una tras otra (mover y luego girar no se cruzan), en un
    // hilo aparte porque los comandos son bloqueantes. Cada respuesta va por el mismo canal que los cambios.
    let (pedidas, mut cola) = mpsc::unbounded_channel::<(u32, serde_json::Value)>();
    let (red2, jugador) = (red.clone(), yo.id);
    tokio::spawn(async move {
        while let Some((id, peticion)) = cola.recv().await {
            let red3 = red2.clone();
            let r = tokio::task::spawn_blocking(move || {
                peticiones::decodificar(peticion).and_then(|p| red3.ejecutar_peticion(jugador, p))
            })
            .await
            .unwrap_or_else(|_| Err("No se pudo hacer".into()));
            let _ = respuestas.send(match r {
                Ok(ok) => MensajeHost::Respuesta { id, ok: Some(ok), error: None },
                Err(error) => MensajeHost::Respuesta { id, ok: None, error: Some(error) },
            });
        }
    });
    let mut limite = LimitePeticiones::nuevo();

    let mut ping = tokio::time::interval(CADA_PING);
    ping.tick().await;
    loop {
        tokio::select! {
            m = salida.recv() => {
                let Some(m) = m else { break };
                let fin = matches!(m, MensajeHost::Rechazado { .. } | MensajeHost::SalaCerrada);
                if enviar(&mut tx, &m).await.is_err() || fin {
                    break;
                }
            }
            entrante = rx.next() => match entrante {
                Some(Ok(Message::Text(t))) => {
                    // Lo que no se entiende se ignora; una acción pasa por el límite y por la cola
                    if let Ok(MensajeCliente::Pedir { id, peticion }) = serde_json::from_str(t.as_str()) {
                        if limite.permite() {
                            let _ = pedidas.send((id, peticion));
                        } else {
                            let _ = avisos.send(MensajeHost::Respuesta { id, ok: None, error: Some("Vas demasiado rápido".into()) });
                        }
                    }
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Binary(_))) => {}
                _ => break,
            },
            _ = ping.tick() => {
                if tx.send(Message::Ping(Bytes::new())).await.is_err() {
                    break;
                }
            }
        }
    }
    red.desconectar(yo.id, id_conexion);
    let _ = tx.close().await;
}

/// Cuenta las peticiones de la última ventana de un segundo.
struct LimitePeticiones {
    desde: std::time::Instant,
    n: u32,
}

impl LimitePeticiones {
    fn nuevo() -> Self {
        Self { desde: std::time::Instant::now(), n: 0 }
    }

    fn permite(&mut self) -> bool {
        if self.desde.elapsed() >= Duration::from_secs(1) {
            *self = Self::nuevo();
        }
        self.n += 1;
        self.n <= PETICIONES_POR_SEGUNDO
    }
}

/// Pide nombre y contraseña hasta que valgan. Registra al jugador en la partida (delta `JugadorRegistrado`) y
/// le da un token para que su navegador entre solo la próxima vez. `None` si hay que cortar.
async fn identificar(red: &Arc<Red>, tx: &mut Emisor, rx: &mut Receptor, pide_contrasena: bool) -> Option<(Jugador, Option<String>)> {
    let mut fallos = 0;
    let mut error = None;
    loop {
        enviar(tx, &MensajeHost::Identificate { pide_contrasena, error: error.take() }).await.ok()?;
        let Ok(Some(MensajeCliente::Unirse { nombre, contrasena })) = timeout(TIEMPO_IDENTIFICARSE, recibir(rx)).await else {
            rechazar(tx, "No se recibió el nombre").await;
            return None;
        };
        if red.demasiados_fallos() {
            rechazar(tx, "Demasiados intentos: espera un minuto").await;
            return None;
        }

        // Argon2 tarda: se comprueba fuera del lock, con una copia de la sala
        let sala = red.dentro().as_ref().map(|s| s.sala.clone());
        let Some(sala) = sala else {
            rechazar(tx, "La sala se ha cerrado").await;
            return None;
        };
        let correcta = tokio::task::spawn_blocking(move || sala.comprobar_contrasena(contrasena.as_deref())).await.unwrap_or(false);
        if !correcta {
            fallos += 1;
            let sigue = red.anotar_fallo();
            tokio::time::sleep(Duration::from_millis(500)).await;
            if fallos >= super::FALLOS_POR_CONEXION || !sigue {
                rechazar(tx, "Demasiados intentos con la contraseña").await;
                return None;
            }
            error = Some("Contraseña incorrecta".to_string());
            continue;
        }

        // Alta en la partida (un delta normal, bajo el lock del estado)
        let alta = {
            let mut est = red.estado_partida();
            jugadores::registrar(&est, &nombre).map(|delta| {
                let DeltaPartida::JugadorRegistrado(j) = &delta else { unreachable!("registrar da de alta") };
                let j = j.clone();
                partida::registrar(&mut est, &red.almacen, delta);
                j
            })
        };
        let jugador = match alta {
            Ok(j) => j,
            Err(e) => {
                error = Some(e); // nombre vacío o largo: no cuenta como fallo
                continue;
            }
        };
        let token = red.cambiar_sala(|s| Ok(s.nuevo_token(jugador.id))).ok();
        return Some((jugador, token));
    }
}

// ------------------------------------------------------------------
// Imágenes
// ------------------------------------------------------------------

#[derive(Deserialize)]
struct PedidoImagen {
    ruta: String,
    /// "ficha" o "mapa": la miniatura en vez del original.
    mini: Option<String>,
    sesion: String,
}

fn tipo_mime(ruta: &str) -> &'static str {
    match ruta.rsplit('.').next().map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("svg") => "image/svg+xml",
        Some("avif") => "image/avif",
        _ => "application/octet-stream",
    }
}

async fn imagen(State(red): State<Arc<Red>>, Query(p): Query<PedidoImagen>) -> Response {
    let Some(jugador) = red.jugador_de_sesion(&p.sesion) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let Some(dir) = red.dir_sala() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let visible = visibilidad::recurso_visible_para(jugador, &red.estado_partida(), &p.ruta)
        || red.subidas_de(jugador).contains(&p.ruta); // lo que acaba de subir, aunque aún no lo use ninguna ficha
    if !visible {
        return StatusCode::NOT_FOUND.into_response();
    }
    let tipo = p.mini.as_deref().and_then(miniaturas::Tipo::desde_texto);
    let ruta = p.ruta;
    let leido = tokio::task::spawn_blocking(move || -> Result<(Vec<u8>, &'static str), String> {
        let final_ = match tipo {
            Some(t) => miniaturas::asegurar(&dir, &ruta, t)?.unwrap_or(ruta),
            None => ruta,
        };
        let camino = archivos::ruta_segura(&dir, &final_, &["imagenes", "miniaturas"])?;
        let datos = std::fs::read(camino).map_err(|e| e.to_string())?;
        Ok((datos, tipo_mime(&final_)))
    })
    .await;
    match leido {
        Ok(Ok((datos, mime))) => {
            let mut r = datos.into_response();
            let c = r.headers_mut();
            c.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
            c.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=3600"));
            c.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
            // Un SVG abierto directamente no puede ejecutar nada
            c.insert("content-security-policy", HeaderValue::from_static("default-src 'none'; style-src 'unsafe-inline'; sandbox"));
            r
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

// ------------------------------------------------------------------
// CORS y subida de fotos
// ------------------------------------------------------------------

/// El origen de la petición, si es el de la página de la sala (o localhost en desarrollo).
fn origen_permitido(red: &Red, cabeceras: &HeaderMap) -> Option<HeaderValue> {
    let o = cabeceras.get(header::ORIGIN)?;
    red.origenes.iter().any(|x| Some(x.as_str()) == o.to_str().ok()).then(|| o.clone())
}

/// CORS para el navegador del jugador: la página está en otra dirección que el túnel. Solo el origen de la
/// página; el OPTIONS (pre-vuelo) se contesta aquí.
async fn cors(State(red): State<Arc<Red>>, peticion: axum::extract::Request, siguiente: axum::middleware::Next) -> Response {
    let origen = origen_permitido(&red, peticion.headers());
    let preflight = peticion.method() == axum::http::Method::OPTIONS;
    let mut r = if preflight { StatusCode::NO_CONTENT.into_response() } else { siguiente.run(peticion).await };
    if let Some(o) = origen {
        let c = r.headers_mut();
        c.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, o);
        c.insert(header::VARY, HeaderValue::from_static("Origin"));
        if preflight {
            c.insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET, POST, OPTIONS"));
            c.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("content-type"));
            c.insert(header::ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("600"));
        }
    }
    r
}

#[derive(Deserialize)]
struct PedidoSubida {
    sesion: String,
    /// Extensión del archivo (png, jpg...). El contenido se comprueba de todos modos.
    ext: String,
}

/// Un jugador sube la foto de una de sus fichas. Solo si tiene alguna, solo imágenes de verdad y con tope. Se
/// guarda como cualquier imagen (por su hash) y se anota como suya: solo ella podrá verla y usarla en su ficha.
async fn subir(State(red): State<Arc<Red>>, cabeceras: HeaderMap, Query(p): Query<PedidoSubida>, cuerpo: Bytes) -> Response {
    if origen_permitido(&red, &cabeceras).is_none() {
        return (StatusCode::FORBIDDEN, "Origen no permitido").into_response();
    }
    let Some(jugador) = red.jugador_de_sesion(&p.sesion) else {
        return (StatusCode::FORBIDDEN, "Sesión no válida").into_response();
    };
    let Some(dir) = red.dir_sala() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !red.estado_partida().banco_entidades.iter().any(|e| e.propietarios.contains(&jugador)) {
        return (StatusCode::FORBIDDEN, "No tienes ninguna ficha").into_response();
    }
    let guardada = tokio::task::spawn_blocking(move || crate::tablero_general::imagenes::importar_bytes(&dir, &cuerpo, &p.ext)).await;
    let ruta = match guardada {
        Ok(Ok(ruta)) => ruta,
        Ok(Err(e)) => return (StatusCode::BAD_REQUEST, e).into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if let Err(e) = red.anotar_subida(jugador, ruta.clone()) {
        return (StatusCode::TOO_MANY_REQUESTS, e).into_response();
    }
    axum::Json(serde_json::json!({ "ruta": ruta })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::red::vista::VistaJugador;
    use crate::tablero_general::almacen::Almacen;
    use crate::tablero_general::almacen::pruebas::dir_temporal;
    use crate::tablero_general::estado::EstadoPartida;
    use futures_util::{SinkExt, StreamExt};
    use std::sync::Mutex;
    use tokio_tungstenite::tungstenite::{self, client::IntoClientRequest};

    type Cliente = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

    async fn red_de_prueba(nombre: &str, contrasena: Option<&str>) -> (Arc<Red>, u16) {
        let dir = dir_temporal(nombre);
        let almacen = Arc::new(Almacen::abrir(&dir).unwrap());
        if let Some(c) = contrasena {
            let mut s = crate::red::sala::SalaGuardada::leer_o_crear(&dir).unwrap();
            s.poner_contrasena(Some(c)).unwrap();
            s.guardar(&dir).unwrap();
        }
        let red = Arc::new(Red::nueva(Arc::new(Mutex::new(EstadoPartida::default())), almacen, dir));
        let puerto = red.abrir_local().await;
        (red, puerto)
    }

    async fn conectar(puerto: u16, origen: &str) -> Result<Cliente, tungstenite::Error> {
        let mut pedido = format!("ws://127.0.0.1:{puerto}/ws").into_client_request().unwrap();
        pedido.headers_mut().insert("origin", origen.parse().unwrap());
        tokio_tungstenite::connect_async(pedido).await.map(|(c, _)| c)
    }

    async fn mandar(c: &mut Cliente, m: &MensajeCliente) {
        c.send(tungstenite::Message::Text(serde_json::to_string(m).unwrap().into())).await.unwrap();
    }

    /// Se conecta y se queda con el reto (lo primero que envía el host).
    async fn conectar_con_reto(puerto: u16, origen: &str) -> (Cliente, String) {
        let mut c = conectar(puerto, origen).await.unwrap();
        let MensajeHost::Reto { reto } = leer(&mut c).await else { panic!("primero va el reto") };
        (c, reto)
    }

    async fn leer(c: &mut Cliente) -> MensajeHost {
        loop {
            match c.next().await.expect("cerrado").unwrap() {
                tungstenite::Message::Text(t) => return serde_json::from_str(t.as_str()).unwrap(),
                _ => continue,
            }
        }
    }

    /// Hola con el enlace bueno; con `token`, respondiendo al reto como el navegador.
    fn hola(red: &Red, reto: &str, token: Option<&str>) -> MensajeCliente {
        let s = red.sala_para_tests();
        let prueba = token.map(|t| crate::red::sala::SalaGuardada::responder(t, reto, "prueba.local"));
        MensajeCliente::Hola { version: VERSION_PROTOCOLO, sala: s.id, secreto: s.secreto, prueba }
    }

    fn origen(red: &Red) -> String {
        red.origenes[0].clone()
    }

    #[tokio::test]
    async fn sin_el_origen_del_cliente_no_se_abre_el_websocket() {
        let (_red, puerto) = red_de_prueba("srv_origen", None).await;
        assert!(conectar(puerto, "https://malo.example").await.is_err());
    }

    #[tokio::test]
    async fn un_enlace_que_no_vale_se_rechaza() {
        let (red, puerto) = red_de_prueba("srv_enlace", None).await;
        let (mut c, _) = conectar_con_reto(puerto, &origen(&red)).await;
        mandar(&mut c, &MensajeCliente::Hola { version: VERSION_PROTOCOLO, sala: "x".into(), secreto: "y".into(), prueba: None }).await;
        assert!(matches!(leer(&mut c).await, MensajeHost::Rechazado { .. }));

        // Otra versión del protocolo, tampoco
        let (mut c, reto) = conectar_con_reto(puerto, &origen(&red)).await;
        let MensajeCliente::Hola { sala, secreto, .. } = hola(&red, &reto, None) else { unreachable!() };
        mandar(&mut c, &MensajeCliente::Hola { version: VERSION_PROTOCOLO + 1, sala, secreto, prueba: None }).await;
        assert!(matches!(leer(&mut c).await, MensajeHost::Rechazado { .. }));
    }

    #[tokio::test]
    async fn con_contrasena_se_registra_y_luego_entra_con_su_token_sin_preguntar() {
        let (red, puerto) = red_de_prueba("srv_contrasena", Some("dragón")).await;
        let (mut c, reto) = conectar_con_reto(puerto, &origen(&red)).await;
        mandar(&mut c, &hola(&red, &reto, None)).await;
        assert_eq!(leer(&mut c).await, MensajeHost::Identificate { pide_contrasena: true, error: None });

        // Mal: vuelve a preguntar
        mandar(&mut c, &MensajeCliente::Unirse { nombre: "Ana".into(), contrasena: Some("otra".into()) }).await;
        assert!(matches!(leer(&mut c).await, MensajeHost::Identificate { error: Some(_), .. }));

        // Bien: bienvenida con token y vista
        mandar(&mut c, &MensajeCliente::Unirse { nombre: "Ana".into(), contrasena: Some("dragón".into()) }).await;
        let MensajeHost::Bienvenido { yo, token: Some(token), .. } = leer(&mut c).await else { panic!("sin bienvenida") };
        assert_eq!(yo, Jugador { id: 1, nombre: "Ana".into() });
        assert_eq!(leer(&mut c).await, MensajeHost::Vista { vista: VistaJugador::default() });
        assert_eq!(red.estado_partida().jugadores, vec![yo.clone()], "queda registrado en la partida");
        assert_eq!(red.conectados(), vec![1]);

        // Otra vez, respondiendo al reto con su token: entra directamente (y la conexión anterior se cierra)
        let (mut c2, reto2) = conectar_con_reto(puerto, &origen(&red)).await;
        mandar(&mut c2, &hola(&red, &reto2, Some(&token))).await;
        let MensajeHost::Bienvenido { yo: yo2, token: None, .. } = leer(&mut c2).await else { panic!("debía entrar") };
        assert_eq!(yo2, yo);
        assert!(matches!(leer(&mut c).await, MensajeHost::Rechazado { .. }), "la pestaña vieja se cierra");

        // Una respuesta para otro reto (copiada de otra conexión) no sirve
        let (mut c4, _) = conectar_con_reto(puerto, &origen(&red)).await;
        mandar(&mut c4, &hola(&red, &reto2, Some(&token))).await;
        assert!(matches!(leer(&mut c4).await, MensajeHost::Identificate { .. }), "se le pregunta como a un desconocido");

        // Tras regenerar el enlace, el viejo no vale (ni con token)
        red.regenerar_enlace().unwrap();
        let (mut c3, reto3) = conectar_con_reto(puerto, &origen(&red)).await;
        let prueba = Some(crate::red::sala::SalaGuardada::responder(&token, &reto3, "prueba.local"));
        mandar(&mut c3, &MensajeCliente::Hola { version: VERSION_PROTOCOLO, sala: "vieja".into(), secreto: "vieja".into(), prueba }).await;
        assert!(matches!(leer(&mut c3).await, MensajeHost::Rechazado { .. }));
    }

    #[tokio::test]
    async fn sin_contrasena_basta_el_nombre_y_un_nombre_vacio_no_vale() {
        let (red, puerto) = red_de_prueba("srv_sin_contrasena", None).await;
        let (mut c, reto) = conectar_con_reto(puerto, &origen(&red)).await;
        mandar(&mut c, &hola(&red, &reto, None)).await;
        assert_eq!(leer(&mut c).await, MensajeHost::Identificate { pide_contrasena: false, error: None });
        mandar(&mut c, &MensajeCliente::Unirse { nombre: "  ".into(), contrasena: None }).await;
        assert!(matches!(leer(&mut c).await, MensajeHost::Identificate { error: Some(_), .. }));
        mandar(&mut c, &MensajeCliente::Unirse { nombre: "Luis".into(), contrasena: None }).await;
        assert!(matches!(leer(&mut c).await, MensajeHost::Bienvenido { .. }));
    }

    #[tokio::test]
    async fn tras_cinco_fallos_se_echa_la_conexion() {
        let (red, puerto) = red_de_prueba("srv_fallos", Some("buena")).await;
        let (mut c, reto) = conectar_con_reto(puerto, &origen(&red)).await;
        mandar(&mut c, &hola(&red, &reto, None)).await;
        leer(&mut c).await;
        for i in 1..=5 {
            mandar(&mut c, &MensajeCliente::Unirse { nombre: "X".into(), contrasena: Some(format!("mala{i}")) }).await;
            let r = leer(&mut c).await;
            if i < 5 {
                assert!(matches!(r, MensajeHost::Identificate { .. }), "{i}: {r:?}");
            } else {
                assert!(matches!(r, MensajeHost::Rechazado { .. }), "{i}: {r:?}");
            }
        }
        assert!(red.estado_partida().jugadores.is_empty());
    }

    #[tokio::test]
    async fn los_cambios_llegan_y_al_cerrar_la_sala_se_avisa() {
        let (red, puerto) = red_de_prueba("srv_cambios", None).await;
        let (mut c, reto) = conectar_con_reto(puerto, &origen(&red)).await;
        mandar(&mut c, &hola(&red, &reto, None)).await;
        leer(&mut c).await;
        mandar(&mut c, &MensajeCliente::Unirse { nombre: "Ana".into(), contrasena: None }).await;
        leer(&mut c).await; // Bienvenido
        leer(&mut c).await; // Vista

        // El host comparte un mapa: le llegan el mapa y su zona
        {
            let mut est = red.estado_partida();
            est.zonas.push(crate::tablero_general::modelos::Zona { id: 1, nombre: "Z".into(), orden: 0 });
            est.mapas.push(crate::tablero_general::modelos::Mapa {
                id: 1, id_zona: 1, nombre: "M".into(), url_imagen_fondo: None, ancho: 1000, alto: 1000, rejilla: 50,
                color_opacidad: vec![255, 255, 255, 76], grosor: 1, orden: 0, compartido: false,
            });
            est.aplicar_delta(&DeltaPartida::MapaCompartido { id: 1, compartido: true });
            red.tras_cambio(&est);
        }
        let MensajeHost::Cambios { cambios } = leer(&mut c).await else { panic!() };
        assert_eq!(cambios.mapas.iter().map(|m| m.id).collect::<Vec<_>>(), vec![1]);
        assert_eq!(cambios.zonas.iter().map(|z| z.id).collect::<Vec<_>>(), vec![1]);

        red.cerrar();
        assert_eq!(leer(&mut c).await, MensajeHost::SalaCerrada);
    }

    #[tokio::test]
    async fn el_fondo_de_un_mapa_compartido_se_sirve_y_el_de_uno_oculto_no() {
        let (red, puerto) = red_de_prueba("srv_imagen", None).await;
        let dir = red.dir_sala().unwrap();
        std::fs::create_dir_all(dir.join("imagenes")).unwrap();
        std::fs::write(dir.join("imagenes/fondo.png"), b"PNG del fondo").unwrap();
        std::fs::write(dir.join("imagenes/oculto.png"), b"PNG oculto").unwrap();
        {
            let mut est = red.estado_partida();
            let mapa = |id, ruta: &str, compartido| crate::tablero_general::modelos::Mapa {
                id, id_zona: 1, nombre: "M".into(), url_imagen_fondo: Some(ruta.into()), ancho: 1000, alto: 1000, rejilla: 50,
                color_opacidad: vec![255, 255, 255, 76], grosor: 1, orden: 0, compartido,
            };
            est.mapas = vec![mapa(1, "imagenes/fondo.png", true), mapa(2, "imagenes/oculto.png", false)];
        }
        let (mut c, reto) = conectar_con_reto(puerto, &origen(&red)).await;
        mandar(&mut c, &hola(&red, &reto, None)).await;
        leer(&mut c).await;
        mandar(&mut c, &MensajeCliente::Unirse { nombre: "Ana".into(), contrasena: None }).await;
        let MensajeHost::Bienvenido { sesion, .. } = leer(&mut c).await else { panic!() };

        let pedir = |ruta: &str, sesion: &str| format!("http://127.0.0.1:{puerto}/imagen?ruta={ruta}&sesion={sesion}");
        let r = reqwest::get(pedir("imagenes%2Ffondo.png", &sesion)).await.unwrap();
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers()["content-type"], "image/png");
        assert_eq!(r.bytes().await.unwrap().as_ref(), b"PNG del fondo");

        assert_eq!(reqwest::get(pedir("imagenes%2Foculto.png", &sesion)).await.unwrap().status(), 404, "mapa no compartido");
        assert_eq!(reqwest::get(pedir("imagenes%2Ffondo.png", "otra")).await.unwrap().status(), 403, "sin sesión válida");

        // La sesión sigue valiendo al reconectar (el navegador ya tiene las URLs)
        drop(c);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(reqwest::get(pedir("imagenes%2Ffondo.png", &sesion)).await.unwrap().status(), 200);
    }

    // ------------------------------------------------------------------
    // Acciones del jugador (peticiones) y subida de fotos
    // ------------------------------------------------------------------

    use crate::tablero_general::modelos::*;
    use crate::tablero_general::partida::{procesar_delta, AppState};
    use serde_json::{json, Value};

    struct Sala {
        /// El estado de la aplicación (con su `Red`), como lo tendría Tauri.
        estado: Arc<AppState>,
        red: Arc<Red>,
        puerto: u16,
        /// Lo que la pantalla del host ha recibido (`partida-cambiada`, `aviso-tablero`).
        eventos: Arc<Mutex<Vec<(String, Value)>>>,
    }

    impl Sala {
        fn estado(&self) -> &AppState {
            &self.estado
        }
    }

    /// Una partida con la sala abierta (sin túnel), el ejecutor de peticiones puesto y el mapa 1 compartido.
    async fn sala_con_app(nombre: &str) -> Sala {
        let dir = dir_temporal(nombre);
        let estado = Arc::new(AppState::nuevo(EstadoPartida::default(), Almacen::abrir(&dir).unwrap(), dir));
        let red = estado.red.clone();

        let eventos = Arc::new(Mutex::new(Vec::new()));
        let guardados = eventos.clone();
        red.poner_emisor(move |e, p| guardados.lock().unwrap().push((e.to_string(), p)));
        let (estado2, red2) = (estado.clone(), red.clone());
        red.poner_ejecutor(move |j, p| peticiones::ejecutar_en(&estado2, &red2, j, p));

        let puerto = red.abrir_local().await;
        let sala = Sala { estado, red, puerto, eventos };
        {
            let e = sala.estado();
            procesar_delta(e, DeltaPartida::ZonaCreada(Zona { id: 1, nombre: "Z".into(), orden: 0 }));
            let mapa = |id| Mapa {
                id, id_zona: 1, nombre: format!("M{id}"), url_imagen_fondo: None, ancho: 1000, alto: 1000, rejilla: 50,
                color_opacidad: vec![255, 255, 255, 76], grosor: 1, orden: 0, compartido: false,
            };
            let zona = Zona { id: 1, nombre: "Z".into(), orden: 0 };
            procesar_delta(e, DeltaPartida::MapaCreado(zona.clone(), mapa(1)));
            procesar_delta(e, DeltaPartida::MapaCreado(zona, mapa(2)));
            procesar_delta(e, DeltaPartida::MapaCompartido { id: 1, compartido: true });
        }
        sala
    }

    /// Entra un jugador sin contraseña. Devuelve su conexión, su id y su sesión (ya con la primera vista leída).
    async fn entrar(sala: &Sala, nombre: &str) -> (Cliente, IdJugador, String) {
        let (mut c, reto) = conectar_con_reto(sala.puerto, &sala.red.origenes[0]).await;
        mandar(&mut c, &hola(&sala.red, &reto, None)).await;
        leer(&mut c).await;
        mandar(&mut c, &MensajeCliente::Unirse { nombre: nombre.into(), contrasena: None }).await;
        let MensajeHost::Bienvenido { yo, sesion, .. } = leer(&mut c).await else { panic!("sin bienvenida") };
        assert!(matches!(leer(&mut c).await, MensajeHost::Vista { .. }));
        (c, yo.id, sesion)
    }

    /// Una ficha del jugador `j` (la 10) con su instancia 1 en el mapa 1, y una del host (la 11, instancia 2).
    fn poner_fichas(sala: &Sala, j: IdJugador) {
        let e = sala.estado();
        let ficha = |id, propietarios: Vec<IdJugador>| Entidad {
            id, nombre: format!("F{id}"), tipo: TipoEntidad::Jugador,
            parametros: parametros_por_defecto().iter().map(|d| Parametro { id_definicion: d.id, valor: d.valor_por_defecto.clone(), publico: true }).collect(),
            cuerpo: Cuerpo::Pequenio, zona_id: 1, foto_url: None, miniatura_url: None, foto_encuadre: Default::default(),
            archivos_adjuntos: vec![], es_unico: false, propietarios,
        };
        let instancia = |id, base: Entidad| Instancia {
            id_instancia: id, id_mapa: Some(1), valores_actuales: vec![], base,
            coordenadas: Coordenada::Casilla(Casilla { x: 1, y: 1 }), activo: true, rotacion: 0, espejo: false, orden: id, capa: 0,
        };
        for (id, p) in [(10, vec![j]), (11, vec![])] {
            procesar_delta(e, DeltaPartida::EntidadCreada(ficha(id, p.clone())));
            procesar_delta(e, DeltaPartida::InstanciaCreada(instancia(id - 9, ficha(id, p))));
        }
    }

    /// Manda una acción y lee hasta su respuesta. Devuelve lo recibido antes (los cambios) y la respuesta.
    async fn pedir(c: &mut Cliente, id: u32, peticion: Value) -> (Vec<MensajeHost>, Result<Value, String>) {
        mandar(c, &MensajeCliente::Pedir { id, peticion }).await;
        let mut antes = Vec::new();
        loop {
            match leer(c).await {
                MensajeHost::Respuesta { id: i, ok, error } if i == id => {
                    return (antes, error.map_or_else(|| Ok(ok.unwrap_or(Value::Null)), Err));
                }
                otro => antes.push(otro),
            }
        }
    }

    /// Lee y descarta lo pendiente hasta que no llegue nada más.
    async fn vaciar(c: &mut Cliente) {
        while let Ok(Some(Ok(_))) = tokio::time::timeout(Duration::from_millis(150), c.next()).await {}
    }

    #[tokio::test]
    async fn un_jugador_tira_el_chat_llega_con_su_nombre_a_todos_y_antes_que_la_respuesta() {
        let sala = sala_con_app("srv_tirada").await;
        let (mut ana, _, _) = entrar(&sala, "Ana").await;
        let (mut luis, _, _) = entrar(&sala, "Luis").await;
        vaciar(&mut ana).await;
        vaciar(&mut luis).await;

        let (antes, r) = pedir(&mut ana, 1, json!({ "cmd": "tirar_dado", "comando": "1d6 + 2" })).await;
        let texto = r.unwrap();
        assert!(texto.as_str().unwrap().contains(" = "), "{texto}");
        // El cambio con el chat llega ANTES que la respuesta
        let chat = antes.iter().find_map(|m| match m {
            MensajeHost::Cambios { cambios } => cambios.historial_tiradas.clone(),
            _ => None,
        });
        let chat = chat.expect("el chat llega como cambio, antes de la respuesta");
        assert_eq!((chat[0].autor.as_str(), chat[0].jugador), ("Ana", 1));

        // Luis también lo ve, con el nombre de Ana
        let MensajeHost::Cambios { cambios } = leer(&mut luis).await else { panic!() };
        assert_eq!(cambios.historial_tiradas.unwrap()[0].autor, "Ana");
        // Y el host se entera
        assert!(sala.eventos.lock().unwrap().iter().any(|(e, p)| e == "partida-cambiada" && p["tiradas"] == true));
        // El historial personal del host no recibe lo que escribe un jugador
        assert!(sala.estado().almacen.leer_historial().is_empty());

        // `clean` no se puede: no está en la lista blanca; y como tirada, tampoco
        assert!(pedir(&mut ana, 2, json!({ "cmd": "limpiar_chat", "comando": "clean" })).await.1.is_err());
        let (_, r) = pedir(&mut ana, 3, json!({ "cmd": "tirar_dado", "comando": "clean" })).await;
        assert!(r.unwrap_err().contains("Solo el host"));
        assert_eq!(sala.estado().lock().historial_tiradas.len(), 1);
        // Una tirada que no es una tirada es un error (no un texto en el chat)
        assert!(pedir(&mut ana, 4, json!({ "cmd": "tirar_dado", "comando": "<b>hola</b>" })).await.1.is_err());
    }

    #[tokio::test]
    async fn un_jugador_mueve_sus_fichas_pero_no_las_de_otros_ni_a_mapas_sin_compartir() {
        let sala = sala_con_app("srv_mover").await;
        let (mut ana, id, _) = entrar(&sala, "Ana").await;
        poner_fichas(&sala, id);
        vaciar(&mut ana).await;

        let casilla = |m: u16, x: u16| json!({ "id_mapa": m, "casilla": { "x": x, "y": 6 } });
        let (antes, r) = pedir(&mut ana, 1, json!({ "cmd": "mover_instancia", "idInstancia": 1, "destino": casilla(1, 7) })).await;
        assert!(r.is_ok(), "{r:?}");
        assert!(antes.iter().any(|m| matches!(m, MensajeHost::Cambios { cambios } if !cambios.instancias.is_empty())));
        let pos = |i: u16| sala.estado().lock().instancias_activas.iter().find(|x| x.id_instancia == i).unwrap().coordenadas;
        assert_eq!(pos(1), Coordenada::Casilla(Casilla { x: 7, y: 6 }));

        let r = pedir(&mut ana, 2, json!({ "cmd": "mover_instancia", "idInstancia": 2, "destino": casilla(1, 9) })).await.1;
        assert!(r.unwrap_err().contains("no es tuya"));
        assert_eq!(pos(2), Coordenada::Casilla(Casilla { x: 1, y: 1 }), "la del host no se ha movido");
        let r = pedir(&mut ana, 3, json!({ "cmd": "mover_instancia", "idInstancia": 1, "destino": casilla(2, 9) })).await.1;
        assert!(r.unwrap_err().contains("no está compartido"));

        // Girar, espejar y cambiar su vida
        assert!(pedir(&mut ana, 4, json!({ "cmd": "rotar_instancia", "idInstancia": 1 })).await.1.is_ok());
        assert!(pedir(&mut ana, 5, json!({ "cmd": "espejar_instancia", "idInstancia": 1 })).await.1.is_ok());
        let i = sala.estado().lock().instancias_activas[0].clone();
        assert_eq!((i.rotacion, i.espejo), (1, true));

        // El host, en cambio, sí puede llevar una ficha a un mapa sin compartir (no pasa por `permitida`)
        let e = sala.estado();
        let r = crate::tablero_general::partida::mover_instancia_en(
            1,
            Some(crate::tablero_general::instancias::Destino { id_mapa: 2, casilla: Casilla { x: 3, y: 3 } }),
            e,
        );
        assert!(r.is_ok());
        assert_eq!(e.lock().instancias_activas[0].id_mapa, Some(2));
    }

    #[tokio::test]
    async fn el_ping_de_un_jugador_lo_ven_el_host_y_los_demas_pero_no_su_foco() {
        let sala = sala_con_app("srv_ping").await;
        let (mut ana, _, _) = entrar(&sala, "Ana").await;
        let (mut luis, _, _) = entrar(&sala, "Luis").await;
        vaciar(&mut ana).await;
        vaciar(&mut luis).await;

        let ping = json!({ "Ping": { "id_mapa": 1, "casilla": { "x": 2, "y": 2 } } });
        assert!(pedir(&mut ana, 1, json!({ "cmd": "avisar_tablero", "aviso": ping })).await.1.is_ok());
        assert!(matches!(leer(&mut luis).await, MensajeHost::Aviso { .. }), "otro jugador lo ve");
        assert!(sala.eventos.lock().unwrap().iter().any(|(e, p)| e == "aviso-tablero" && p["Ping"]["id_mapa"] == 1), "el host lo ve");

        // En un mapa que no se comparte, no
        let oculto = json!({ "Ping": { "id_mapa": 2, "casilla": { "x": 2, "y": 2 } } });
        assert!(pedir(&mut ana, 2, json!({ "cmd": "avisar_tablero", "aviso": oculto })).await.1.is_err());

        // El foco se acepta y no llega a nadie
        let antes = sala.eventos.lock().unwrap().len();
        assert!(pedir(&mut ana, 3, json!({ "cmd": "avisar_tablero", "aviso": { "Foco": { "ids_instancias": [1] } } })).await.1.is_ok());
        assert_eq!(sala.eventos.lock().unwrap().len(), antes, "el host no ve el foco");
        assert!(tokio::time::timeout(Duration::from_millis(200), luis.next()).await.is_err(), "ni los demás");
    }

    #[tokio::test]
    async fn demasiadas_peticiones_seguidas_se_contestan_con_un_error() {
        let sala = sala_con_app("srv_limite").await;
        let (mut ana, _, _) = entrar(&sala, "Ana").await;
        for id in 0..(PETICIONES_POR_SEGUNDO + 10) {
            mandar(&mut ana, &MensajeCliente::Pedir { id, peticion: json!({ "cmd": "nada" }) }).await;
        }
        let mut rapidas = 0;
        for _ in 0..(PETICIONES_POR_SEGUNDO + 10) {
            if let MensajeHost::Respuesta { error: Some(e), .. } = leer(&mut ana).await {
                rapidas += e.contains("demasiado rápido") as u32;
            }
        }
        assert!(rapidas >= 5, "las pasadas del límite fallan: {rapidas}");
    }

    /// Un PNG de verdad (2×2).
    fn png() -> Vec<u8> {
        let mut v = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(2, 2).write_to(&mut v, image::ImageFormat::Png).unwrap();
        v.into_inner()
    }

    #[tokio::test]
    async fn un_jugador_sube_la_foto_de_su_ficha_con_cors_y_solo_ve_lo_que_sube() {
        let sala = sala_con_app("srv_subida").await;
        let (_ana, id, sesion) = entrar(&sala, "Ana").await;
        let (_luis, _, sesion_luis) = entrar(&sala, "Luis").await;
        poner_fichas(&sala, id);
        let origen = sala.red.origenes[0].clone();
        let url = |camino: &str| format!("http://127.0.0.1:{}{camino}", sala.puerto);
        let cliente = reqwest::Client::new();
        let subir = |sesion: &str, ext: &str, cuerpo: Vec<u8>, con_origen: bool| {
            let mut r = cliente.post(url(&format!("/subir?sesion={sesion}&ext={ext}"))).body(cuerpo);
            if con_origen {
                r = r.header("origin", origen.clone());
            }
            r.send()
        };

        assert_eq!(subir(&sesion, "png", png(), false).await.unwrap().status(), 403, "sin origen de la página");
        assert_eq!(subir("otra", "png", png(), true).await.unwrap().status(), 403, "sin sesión");
        assert_eq!(subir(&sesion_luis, "png", png(), true).await.unwrap().status(), 403, "Luis no tiene ficha");
        assert_eq!(subir(&sesion, "png", b"esto no es una imagen".to_vec(), true).await.unwrap().status(), 400);
        assert_eq!(subir(&sesion, "svg", png(), true).await.unwrap().status(), 400, "SVG no");
        assert_eq!(subir(&sesion, "jpg", png(), true).await.unwrap().status(), 400, "no es del tipo que dice");

        let r = subir(&sesion, "png", png(), true).await.unwrap();
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers()["access-control-allow-origin"], origen.as_str());
        let ruta = r.json::<Value>().await.unwrap()["ruta"].as_str().unwrap().to_string();
        assert!(ruta.starts_with("imagenes/") && ruta.ends_with(".png"));

        // La ve quien la subió (con CORS, para recortarla en un canvas) y nadie más: aún no la usa ninguna ficha
        let pedir_imagen = |sesion: &str| cliente.get(url(&format!("/imagen?ruta={ruta}&sesion={sesion}"))).header("origin", origen.clone()).send();
        let r = pedir_imagen(&sesion).await.unwrap();
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers()["access-control-allow-origin"], origen.as_str());
        assert_eq!(r.bytes().await.unwrap().as_ref(), png().as_slice());
        assert_eq!(pedir_imagen(&sesion_luis).await.unwrap().status(), 404);

        // El pre-vuelo del navegador
        let r = cliente.request(reqwest::Method::OPTIONS, url("/subir")).header("origin", origen.clone()).send().await.unwrap();
        assert_eq!(r.status(), 204);
        assert_eq!(r.headers()["access-control-allow-origin"], origen.as_str());
        let r = cliente.request(reqwest::Method::OPTIONS, url("/subir")).header("origin", "https://malo.example").send().await.unwrap();
        assert!(r.headers().get("access-control-allow-origin").is_none(), "otro origen, sin permiso");
    }

    #[tokio::test]
    async fn editar_su_ficha_con_la_foto_subida_llega_a_la_partida() {
        let sala = sala_con_app("srv_editar").await;
        let (mut ana, id, sesion) = entrar(&sala, "Ana").await;
        poner_fichas(&sala, id);
        vaciar(&mut ana).await;

        let origen = sala.red.origenes[0].clone();
        let r = reqwest::Client::new()
            .post(format!("http://127.0.0.1:{}/subir?sesion={sesion}&ext=png", sala.puerto))
            .header("origin", origen)
            .body(png())
            .send()
            .await
            .unwrap();
        let foto = r.json::<Value>().await.unwrap()["ruta"].as_str().unwrap().to_string();

        let guardada = sala.estado().lock().banco_entidades.iter().find(|e| e.id == 10).unwrap().clone();
        let editar = |f: &dyn Fn(&mut Entidad)| {
            let mut nueva = guardada.clone();
            f(&mut nueva);
            json!({ "cmd": "modificar_entidad", "entidad": serde_json::to_value(&nueva).unwrap() })
        };
        let (_, r) = pedir(&mut ana, 1, editar(&|f| { f.nombre = "Ana la Roja".into(); f.foto_url = Some(foto.clone()); })).await;
        assert!(r.is_ok(), "{r:?}");
        let e = sala.estado().lock().banco_entidades.iter().find(|e| e.id == 10).unwrap().clone();
        assert_eq!((e.nombre.as_str(), e.foto_url.as_deref()), ("Ana la Roja", Some(foto.as_str())));
        // La instancia también la lleva (la ficha cambió en el banco y en el tablero)
        assert_eq!(sala.estado().lock().instancias_activas[0].base.nombre, "Ana la Roja");

        // Lo que solo es del host, no
        let cambios: [fn(&mut Entidad); 3] = [|f| f.zona_id = 9, |f| f.es_unico = true, |f| f.propietarios = vec![]];
        for (n, cambio) in cambios.iter().enumerate() {
            assert!(pedir(&mut ana, 2 + n as u32, editar(cambio)).await.1.is_err());
        }
    }
}
