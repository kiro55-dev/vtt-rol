// src-tauri/src/red/servidor.rs
//
// El servidor propio del host, en 127.0.0.1 (desde fuera solo se llega por el túnel):
//  * GET /ws       el WebSocket de cada jugador: saludo (ver protocolo.rs) y luego su vista y sus cambios.
//  * GET /imagen   fondos de mapa, fotos y miniaturas, solo si el jugador de esa sesión las ve
//                  (`visibilidad::recurso_visible_para`).
//
// Seguridad del saludo:
//  * Solo se acepta el WebSocket desde el origen del cliente web (`Red::origenes`).
//  * Sin el `id` y el `secreto` del enlace no se pasa del primer mensaje.
//  * Contraseña: 5 fallos por conexión, 20 por minuto en total, y medio segundo de espera tras cada fallo.
//  * Mensajes pequeños (16 KiB) y tiempo límite para saludar.
use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

use crate::red::Red;
use crate::red::protocolo::{MensajeCliente, MensajeHost, VERSION_PROTOCOLO};
use crate::tablero_general::estado::DeltaPartida;
use crate::tablero_general::modelos::Jugador;
use crate::tablero_general::{archivos, jugadores, miniaturas, partida, visibilidad};

const MAX_MENSAJE: usize = 16 * 1024;
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
    let id_conexion = {
        let est = red.estado_partida(); // estado → dentro
        red.conectar(&est, yo.id, canal)
    };
    let Some(id_conexion) = id_conexion else {
        return rechazar(&mut tx, "La sala se ha cerrado").await;
    };
    red.avisar();

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
                // FUTURA RED (fase 2): aquí llegarán las acciones del jugador (mover sus fichas, tiradas...)
                Some(Ok(Message::Text(_) | Message::Ping(_) | Message::Pong(_) | Message::Binary(_))) => {}
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
    let visible = visibilidad::recurso_visible_para(jugador, &red.estado_partida(), &p.ruta);
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
}
