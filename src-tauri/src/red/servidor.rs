// src-tauri/src/red/servidor.rs
//
// El servidor propio del host, en 127.0.0.1 (desde fuera solo se llega por el túnel):
//  * GET /ws       el WebSocket de cada jugador: saludo (ver protocolo.rs) y luego su vista, sus cambios y las
//                  imágenes (pedirlas y subirlas, en trozos: red/imagenes.rs). `atender` no sabe que es un
//                  WebSocket: trabaja sobre un `Transporte` (red/transporte.rs), igual que hará con WebRTC.
//  * GET /imagen   respaldo para navegadores sin Service Worker: la misma imagen que `PedirImagen`, con la misma
//                  regla (`imagenes::localizar`), pidiéndola con la sesión de imágenes del jugador.
//  Como el navegador está en otra dirección (GitHub Pages), /imagen lleva CORS para el origen de la página (el
//  mismo que acepta el WebSocket) y el servidor contesta al OPTIONS.
//
// Seguridad del saludo:
//  * Solo se acepta el WebSocket desde el origen del cliente web (`Red::origenes`).
//  * Sin el `id` y el `secreto` del enlace no se pasa del primer mensaje.
//  * Contraseña: 5 fallos por conexión, 20 por minuto en total, y medio segundo de espera tras cada fallo.
//  * Mensajes de hasta 512 KiB (un .txt largo, una miniatura), tiempo límite para saludar y un tope de peticiones
//    por segundo (las acciones del jugador se hacen en orden, una a una).
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

use crate::red::Red;
use crate::red::imagenes;
use crate::red::pagina;
use crate::red::transporte::{self, Trama, Transporte};
use crate::red::peticiones;
use crate::red::protocolo::{MensajeCliente, MensajeHost, VERSION_PROTOCOLO};
use crate::tablero_general::estado::DeltaPartida;
use crate::tablero_general::modelos::Jugador;
use crate::tablero_general::tablero::AvisoTablero;
use crate::tablero_general::{jugadores, partida};

const MAX_MENSAJE: usize = 512 * 1024;
/// Peticiones por segundo de un jugador (arrastrar, tirar, editar...). Con más, se contestan con un error.
const PETICIONES_POR_SEGUNDO: u32 = 30;
const TIEMPO_SALUDO: Duration = Duration::from_secs(15);
/// Tiempo para escribir nombre y contraseña.
const TIEMPO_IDENTIFICARSE: Duration = Duration::from_secs(180);
/// Peticiones de un jugador esperando a hacerse. Con la cola llena se contesta «vas demasiado rápido».
const MAX_PETICIONES_EN_COLA: usize = 64;
/// Mensajes para un jugador sin enviar todavía (su conexión no da abasto o no lee). Pasado esto se le corta: al
/// reconectar recibe su vista entera, así que no pierde nada, y el host no acumula memoria por él.
const MAX_MENSAJES_PENDIENTES: usize = 2000;

/// Arranca el servidor en `ip`:`puerto` (127.0.0.1 normalmente; 0.0.0.0 en red local; puerto 0 = uno libre).
/// Devuelve el puerto y cómo apagarlo.
pub async fn arrancar(red: Arc<Red>, ip: std::net::Ipv4Addr, puerto: u16) -> Result<(u16, oneshot::Sender<()>), String> {
    let escucha = tokio::net::TcpListener::bind((ip, puerto)).await.map_err(|e| {
        if puerto != 0 && e.kind() == std::io::ErrorKind::AddrInUse {
            format!("El puerto {puerto} está ocupado por otro programa: elige otro en «Cómo se publica»")
        } else {
            format!("No se pudo arrancar el servidor de la sala: {e}")
        }
    })?;
    let puerto = escucha.local_addr().map_err(|e| e.to_string())?.port();
    let app = Router::new()
        .route("/ws", get(ws))
.route("/imagen", get(imagen))
        // Todo lo demás: la página de los jugadores, que va dentro de la app (red/pagina.rs)
        .fallback(get(pagina::servir))
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
    if !red.acepta_origen(origen) {
        return (StatusCode::FORBIDDEN, "Origen no permitido").into_response();
    }
    ws.max_message_size(MAX_MENSAJE).on_upgrade(move |socket| atender(red, transporte::desde_websocket(socket)))
}

type Emisor = mpsc::Sender<Trama>;
type Receptor = mpsc::Receiver<Trama>;

async fn enviar(tx: &Emisor, m: &MensajeHost) -> Result<(), ()> {
    let json = serde_json::to_string(m).map_err(|_| ())?;
    // El arrastre en vivo puede perderse o desordenarse (lleva su número): por la conexión directa va sin reintentos
    let efimero = matches!(m, MensajeHost::Aviso { aviso: AvisoTablero::Arrastre { .. } });
    transporte::enviar_trama(tx, if efimero { Trama::Efimero(json) } else { Trama::Texto(json) }).await
}

/// El siguiente mensaje del cliente (los trozos binarios se saltan). `None` si se cerró o mandó algo que no se
/// entiende.
async fn recibir(rx: &mut Receptor) -> Option<MensajeCliente> {
    loop {
        match rx.recv().await? {
            Trama::Texto(t) | Trama::Efimero(t) => return serde_json::from_str(&t).ok(),
            Trama::Binario(_) => continue,
        }
    }
}

/// Avisa del motivo. La conexión se cierra al soltar el transporte (al salir de `atender`).
async fn rechazar(tx: &Emisor, motivo: &str) {
    let _ = enviar(tx, &MensajeHost::Rechazado { motivo: motivo.into() }).await;
}

/// Atiende a un jugador de principio a fin, venga por donde venga (`Transporte`).
pub(crate) async fn atender(red: Arc<Red>, transporte: Transporte) {
    let Transporte { tx, mut rx } = transporte;
    // Mientras no es nadie ocupa una plaza: así no se pueden abrir cientos de conexiones que se quedan esperando
    let Some(plaza) = red.plaza_saludo() else {
        return rechazar(&tx, "Hay demasiada gente entrando a la vez: prueba en un momento").await;
    };

    // 1. Reto y Hola: el enlace (y la prueba, si el navegador ya entró antes)
    let reto = crate::red::sala::reto();
    if enviar(&tx, &MensajeHost::Reto { reto: reto.clone() }).await.is_err() {
        return;
    }
    let Ok(Some(MensajeCliente::Hola { version, sala, secreto, prueba })) = timeout(TIEMPO_SALUDO, recibir(&mut rx)).await else {
        return rechazar(&tx, "Saludo no válido").await;
    };
    if version != VERSION_PROTOCOLO {
        return rechazar(&tx, "Versión distinta de la del host: recarga la página").await;
    }
    let (valido, recordado, pide_contrasena) = {
        let dentro = red.dentro();
        match dentro.as_ref() {
            Some(s) => (
                s.sin_llave() || s.sala.coincide(&sala, &secreto),
                // La prueba se calcula con la dirección por la que llegó el jugador, y solo valen las de ESTE host
                // (su túnel, su dirección propia o sus IPs en red local): una hecha para otro servidor no vale aquí
                prueba.as_deref().and_then(|p| s.direcciones().iter().find_map(|d| s.sala.jugador_de_prueba(&reto, d, p))),
                s.sala.pide_contrasena(),
            ),
            None => (false, None, false),
        }
    };
    if !valido {
        return rechazar(&tx, "Este enlace ya no vale: pide uno nuevo al host").await;
    }
    // La clave debe ser de un jugador que sigue en la partida
    let recordado = recordado.and_then(|id| red.estado_partida().jugadores.iter().find(|j| j.id == id).cloned());

    // 2. Si no se le recuerda: nombre y contraseña
    let (yo, token_nuevo) = match recordado {
        Some(j) => (j, None),
        None => match identificar(&red, &tx, &mut rx, pide_contrasena).await {
            Some(r) => r,
            None => return,
        },
    };

    drop(plaza);

    // 3. Bienvenida y vista inicial. A partir de aquí, todo lo que se le envía pasa por su canal.
    let Some(sesion) = red.sesion_de(yo.id) else {
        return rechazar(&tx, "La sala se ha cerrado").await;
    };
    let bienvenida = MensajeHost::Bienvenido { yo: yo.clone(), token: token_nuevo, sesion: sesion.clone() };
    if enviar(&tx, &bienvenida).await.is_err() {
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
        return rechazar(&tx, "La sala se ha cerrado").await;
    };
    red.avisar();

    // Las acciones del jugador se ejecutan en orden, una tras otra (mover y luego girar no se cruzan), en un
    // hilo aparte porque los comandos son bloqueantes. Cada respuesta va por el mismo canal que los cambios.
    let (pedidas, mut cola) = mpsc::channel::<(u32, serde_json::Value)>(MAX_PETICIONES_EN_COLA);
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
    // Imágenes por esta misma conexión (ver red/imagenes.rs). Al salir se cancelan las que estén a medias.
    let bajadas = imagenes::Bajadas::default();
    let mut subidas = imagenes::Subidas::default();
    let mut repaso = tokio::time::interval(Duration::from_secs(10));

    loop {
        tokio::select! {
            m = salida.recv() => {
                let Some(m) = m else { break };
                let fin = matches!(m, MensajeHost::Rechazado { .. } | MensajeHost::SalaCerrada);
                // `enviar` espera si el jugador no lee al ritmo (contrapresión) y se rinde a los `TIEMPO_ENVIO`
                if enviar(&tx, &m).await.is_err() || fin || salida.len() > MAX_MENSAJES_PENDIENTES {
                    break;
                }
            }
            entrante = rx.recv() => match entrante {
                // Lo que no se entiende se ignora
                Some(Trama::Texto(t) | Trama::Efimero(t)) => match serde_json::from_str::<MensajeCliente>(&t) {
                    // El arrastre en vivo: sin respuesta ni cola (no puede esperar detrás de una acción lenta), con
                    // las mismas reglas que `avisar_tablero`. Si llega desordenado, lo arregla su número
                    Ok(MensajeCliente::Aviso { aviso }) => {
                        if limite.permite() {
                            let (r, jugador) = (red.clone(), yo.id);
                            tokio::task::spawn_blocking(move || {
                                let _ = r.ejecutar_peticion(jugador, peticiones::Peticion::AvisarTablero { aviso });
                            });
                        }
                    }
                    // Una acción pasa por el límite y por la cola
                    Ok(MensajeCliente::Pedir { id, peticion }) => {
                        let aceptada = limite.permite() && pedidas.try_send((id, peticion)).is_ok();
                        if !aceptada {
                            let _ = avisos.send(MensajeHost::Respuesta { id, ok: None, error: Some("Vas demasiado rápido".into()) });
                        }
                    }
                    // Las imágenes tienen su propio tope (en marcha y en cola): un banco lleno de miniaturas las
                    // pide todas de golpe
                    Ok(MensajeCliente::PedirImagen { id, ruta, mini }) => bajadas.pedir(red.clone(), yo.id, id, ruta, mini, tx.clone()),
                    Ok(MensajeCliente::CancelarImagen { id }) => bajadas.cancelar(id),
                    Ok(MensajeCliente::SubirImagen { id, ext, tam }) => {
                        let empezada = if limite.permite() { subidas.empezar(id, ext, tam) } else { Err("Vas demasiado rápido".into()) };
                        if let Err(error) = empezada {
                            let _ = avisos.send(MensajeHost::SubidaError { id, error });
                        }
                    }
                    _ => {}
                },
                Some(Trama::Binario(b)) => match subidas.trozo(&b) {
                    Some(imagenes::FinSubida::Completa(id, datos, ext)) => {
                        let (r, aviso, jugador) = (red.clone(), avisos.clone(), yo.id);
                        tokio::spawn(async move {
                            let guardada = tokio::task::spawn_blocking(move || imagenes::guardar_subida(&r, jugador, &datos, &ext)).await;
                            let _ = aviso.send(match guardada {
                                Ok(Ok(ruta)) => MensajeHost::SubidaOk { id, ruta },
                                Ok(Err(error)) => MensajeHost::SubidaError { id, error },
                                Err(_) => MensajeHost::SubidaError { id, error: "No se pudo guardar la imagen".into() },
                            });
                        });
                    }
                    Some(imagenes::FinSubida::Mal(id, error)) => {
                        let _ = avisos.send(MensajeHost::SubidaError { id, error });
                    }
                    None => {}
                },
                None => break,
            },
            _ = repaso.tick() => {
                for id in subidas.caducar() {
                    let _ = avisos.send(MensajeHost::SubidaError { id, error: "La subida se ha cortado".into() });
                }
            }
        }
    }
    red.desconectar(yo.id, id_conexion);
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
async fn identificar(red: &Arc<Red>, tx: &Emisor, rx: &mut Receptor, pide_contrasena: bool) -> Option<(Jugador, Option<String>)> {
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

async fn imagen(State(red): State<Arc<Red>>, Query(p): Query<PedidoImagen>) -> Response {
    let Some(jugador) = red.jugador_de_sesion(&p.sesion) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let r = red.clone();
    let leido = tokio::task::spawn_blocking(move || {
        let (camino, mime) = imagenes::localizar(&r, jugador, &p.ruta, p.mini.as_deref())?;
        std::fs::read(camino).ok().map(|datos| (datos, mime))
    })
    .await;
    match leido {
        Ok(Some((datos, mime))) => {
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
    red.acepta_origen(o.to_str().ok()?).then(|| o.clone())
}

/// CORS para el navegador del jugador: la página está en otra dirección que el túnel. Solo el origen de la
/// página; el OPTIONS (pre-vuelo) se contesta aquí.
async fn cors(State(red): State<Arc<Red>>, peticion: axum::extract::Request, siguiente: axum::middleware::Next) -> Response {
    let origen = origen_permitido(&red, peticion.headers());
    let preflight = peticion.method() == axum::http::Method::OPTIONS;
    let mut r = if preflight { StatusCode::NO_CONTENT.into_response() } else { siguiente.run(peticion).await };
    // Siempre `Vary: Origin`: así el navegador no reutiliza para un fetch con CORS la copia que dejó un <img> (sin CORS)
    r.headers_mut().insert(header::VARY, HeaderValue::from_static("Origin"));
    if let Some(o) = origen {
        let c = r.headers_mut();
        c.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, o);
        if preflight {
            c.insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET, OPTIONS"));
            c.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("content-type"));
            c.insert(header::ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("600"));
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
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
            coordenadas: Coordenada::Casilla(Casilla { x: 1, y: 1 }), activo: true, rotacion: 0, espejo: false, orden: id, capa: 0, atenuada: false,
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
    async fn el_arrastre_como_aviso_sin_respuesta_llega_a_los_demas_y_el_de_una_ficha_ajena_no() {
        let sala = sala_con_app("srv_aviso").await;
        let (mut ana, id, _) = entrar(&sala, "Ana").await;
        let (mut luis, _, _) = entrar(&sala, "Luis").await;
        poner_fichas(&sala, id); // la instancia 1 es de Ana; la 2, del host
        vaciar(&mut ana).await;
        vaciar(&mut luis).await;

        let arrastre = |ficha: u16, n: u32| {
            MensajeCliente::Aviso {
                aviso: serde_json::from_value(json!({ "Arrastre": { "id_mapa": 1, "piezas": [{ "id_instancia": ficha, "casilla": { "x": 3, "y": 2 } }], "n": n } })).unwrap(),
            }
        };
        // Una de una ficha ajena se descarta en silencio; la suya llega a Luis, con su número, sin respuesta para Ana
        mandar(&mut ana, &arrastre(2, 1)).await;
        mandar(&mut ana, &arrastre(1, 2)).await;
        let MensajeHost::Aviso { aviso } = leer(&mut luis).await else { panic!("Luis no vio el arrastre") };
        assert!(matches!(aviso, crate::tablero_general::tablero::AvisoTablero::Arrastre { n: 2, .. }), "{aviso:?}");
        assert!(sala.eventos.lock().unwrap().iter().any(|(e, p)| e == "aviso-tablero" && p["Arrastre"]["piezas"][0]["id_instancia"] == 1 && p["Arrastre"]["n"] == 2), "el host lo ve");
        assert!(!sala.eventos.lock().unwrap().iter().any(|(e, p)| e == "aviso-tablero" && p["Arrastre"]["piezas"][0]["id_instancia"] == 2), "el de la ajena no");
    }

    #[tokio::test]
    async fn el_arrastre_de_una_ficha_suya_lo_ven_el_host_y_los_demas_y_el_de_una_ajena_no() {
        let sala = sala_con_app("srv_arrastre").await;
        let (mut ana, id, _) = entrar(&sala, "Ana").await;
        let (mut luis, _, _) = entrar(&sala, "Luis").await;
        poner_fichas(&sala, id); // la instancia 1 es de Ana; la 2, del host
        vaciar(&mut ana).await;
        vaciar(&mut luis).await;

        let arrastre = |ficha: u16| json!({ "Arrastre": { "id_mapa": 1, "piezas": [{ "id_instancia": ficha, "casilla": { "x": 3, "y": 2 } }] } });
        assert!(pedir(&mut ana, 1, json!({ "cmd": "avisar_tablero", "aviso": arrastre(1) })).await.1.is_ok());
        let MensajeHost::Aviso { aviso } = leer(&mut luis).await else { panic!("Luis no vio el arrastre") };
        assert!(matches!(aviso, crate::tablero_general::tablero::AvisoTablero::Arrastre { .. }));
        assert!(sala.eventos.lock().unwrap().iter().any(|(e, p)| e == "aviso-tablero" && p["Arrastre"]["piezas"][0]["id_instancia"] == 1), "el host lo ve");

        // Soltar: el fin también llega; y la posición real no se tocó
        let fin = json!({ "ArrastreFin": { "id_mapa": 1, "ids_instancias": [1] } });
        assert!(pedir(&mut ana, 2, json!({ "cmd": "avisar_tablero", "aviso": fin })).await.1.is_ok());
        assert!(matches!(leer(&mut luis).await, MensajeHost::Aviso { aviso: crate::tablero_general::tablero::AvisoTablero::ArrastreFin { .. } }));
        let est = sala.estado().lock();
        assert_eq!(est.instancias_activas[0].coordenadas, Coordenada::Casilla(Casilla { x: 1, y: 1 }));
        drop(est);

        // Una ficha del host no se puede «arrastrar» en pantalla ajena
        assert!(pedir(&mut ana, 3, json!({ "cmd": "avisar_tablero", "aviso": arrastre(2) })).await.1.is_err());
    }

    #[tokio::test]
    async fn el_historial_de_dados_de_cada_jugador_se_guarda_en_el_host_y_se_recupera_al_reconectar() {
        let sala = sala_con_app("srv_historial").await;
        let (mut ana, id_ana, _) = entrar(&sala, "Ana").await;
        let (mut luis, _, _) = entrar(&sala, "Luis").await;
        for (n, c) in ["1d6 + 2", "2d8", "2d8"].iter().enumerate() {
            assert!(pedir(&mut ana, n as u32, json!({ "cmd": "tirar_dado", "comando": c })).await.1.is_ok());
        }
        let (_, r) = pedir(&mut ana, 10, json!({ "cmd": "obtener_historial_personal" })).await;
        assert_eq!(r.unwrap(), json!(["1d6 + 2", "2d8"]), "sin repetir el último");
        let (_, r) = pedir(&mut luis, 1, json!({ "cmd": "obtener_historial_personal" })).await;
        assert_eq!(r.unwrap(), json!([]), "el de Ana no es suyo");

        // Quitar a Ana de la partida borra su historial
        crate::red::comandos::eliminar_jugador_en(sala.estado(), id_ana).unwrap();
        assert!(sala.estado().almacen.leer_historial_de(id_ana).is_empty());
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
    async fn una_foto_subida_que_no_usa_nadie_se_borra_al_cerrar_la_sala() {
        let sala = sala_con_app("srv_subida_huerfana").await;
        let (_ana, id, _) = entrar(&sala, "Ana").await;
        let ruta = imagenes::guardar_subida(&sala.red, id, &png(), "png").unwrap();
        let archivo = sala.estado().almacen.dir().join(&ruta);
        assert!(archivo.is_file());
        sala.red.cerrar();
        assert!(!archivo.exists(), "no la usaba ninguna ficha: se borra");
    }

    #[tokio::test]
    async fn no_se_pueden_abrir_conexiones_sin_identificar_sin_fin() {
        let (red, puerto) = red_de_prueba("srv_sin_identificar", None).await;
        let mut esperando = Vec::new();
        for _ in 0..crate::red::MAX_SIN_IDENTIFICAR {
            esperando.push(conectar_con_reto(puerto, &origen(&red)).await);
        }
        let mut otra = conectar(puerto, &origen(&red)).await.unwrap();
        assert!(matches!(leer(&mut otra).await, MensajeHost::Rechazado { .. }), "ya hay demasiadas esperando");
        drop(esperando);
        // Al irse, las plazas se liberan (dale un momento al servidor para ver que se cerraron)
        let mut entra = None;
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let mut c = conectar(puerto, &origen(&red)).await.unwrap();
            if matches!(leer(&mut c).await, MensajeHost::Reto { .. }) {
                entra = Some(c);
                break;
            }
        }
        assert!(entra.is_some(), "se puede volver a entrar");
    }

    #[tokio::test]
    async fn la_foto_subida_solo_la_ve_quien_la_subio_y_el_respaldo_http_lleva_cors() {
        let sala = sala_con_app("srv_subida").await;
        let (_ana, id, sesion) = entrar(&sala, "Ana").await;
        let (_luis, _, sesion_luis) = entrar(&sala, "Luis").await;
        poner_fichas(&sala, id);
        let origen = sala.red.origenes[0].clone();
        let url = |camino: &str| format!("http://127.0.0.1:{}{camino}", sala.puerto);
        let cliente = reqwest::Client::new();
        // Las comprobaciones de la subida (solo imágenes de verdad, tamaño...) están en la de por el protocolo
        let ruta = imagenes::guardar_subida(&sala.red, id, &png(), "png").unwrap();
        assert!(ruta.starts_with("imagenes/") && ruta.ends_with(".png"));
        assert!(imagenes::guardar_subida(&sala.red, id, &png(), "svg").is_err(), "SVG no");

        // La ve quien la subió (con CORS, para recortarla en un canvas) y nadie más: aún no la usa ninguna ficha
        let pedir_imagen = |sesion: &str| cliente.get(url(&format!("/imagen?ruta={ruta}&sesion={sesion}"))).header("origin", origen.clone()).send();
        let r = pedir_imagen(&sesion).await.unwrap();
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers()["access-control-allow-origin"], origen.as_str());
        assert_eq!(r.bytes().await.unwrap().as_ref(), png().as_slice());
        assert_eq!(pedir_imagen(&sesion_luis).await.unwrap().status(), 404);
        // Con o sin `Origin` lleva `Vary: Origin`: el navegador no reutiliza para un fetch con CORS la copia de un <img>
        let sin_origen = cliente.get(url(&format!("/imagen?ruta={ruta}&sesion={sesion}"))).send().await.unwrap();
        assert_eq!(sin_origen.status(), 200);
        assert_eq!(sin_origen.headers()["vary"], "Origin");
        assert!(sin_origen.headers().get("access-control-allow-origin").is_none());

        // El pre-vuelo del navegador
        let r = cliente.request(reqwest::Method::OPTIONS, url("/imagen")).header("origin", origen.clone()).send().await.unwrap();
        assert_eq!(r.status(), 204);
        assert_eq!(r.headers()["access-control-allow-origin"], origen.as_str());
        let r = cliente.request(reqwest::Method::OPTIONS, url("/imagen")).header("origin", "https://malo.example").send().await.unwrap();
        assert!(r.headers().get("access-control-allow-origin").is_none(), "otro origen, sin permiso");
    }

    #[tokio::test]
    async fn editar_su_ficha_con_la_foto_subida_llega_a_la_partida() {
        let sala = sala_con_app("srv_editar").await;
        let (mut ana, id, _) = entrar(&sala, "Ana").await;
        poner_fichas(&sala, id);
        vaciar(&mut ana).await;

        let foto = imagenes::guardar_subida(&sala.red, id, &png(), "png").unwrap();

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

    #[tokio::test]
    async fn un_jugador_crea_su_ficha_con_un_txt_y_queda_a_su_nombre() {
        let sala = sala_con_app("srv_ficha_nueva").await;
        let (mut ana, id, _) = entrar(&sala, "Ana").await;
        vaciar(&mut ana).await;

        // Un borrador: el host le da un id y la zona (la que ve)
        let (_, r) = pedir(&mut ana, 1, json!({ "cmd": "iniciar_borrador_entidad", "zonaId": 99, "fotoUrl": null })).await;
        let borrador: Entidad = serde_json::from_value(r.unwrap()).unwrap();
        assert_eq!((borrador.zona_id, borrador.propietarios.clone()), (1, vec![id]));
        assert!(sala.estado().lock().banco_entidades.is_empty(), "aún no existe en el banco");

        // Un .txt en la carpeta de la ficha nueva
        let (_, r) = pedir(&mut ana, 2, json!({ "cmd": "crear_documento_texto", "entidad": serde_json::to_value(&borrador).unwrap() })).await;
        let adjunto = r.unwrap();
        let ruta = adjunto["ruta"].as_str().unwrap().to_string();
        assert!(ruta.starts_with(&format!("archivos/entidad_{}/", borrador.id)) && ruta.ends_with(".txt"), "{ruta}");
        let (_, r) = pedir(&mut ana, 3, json!({ "cmd": "guardar_archivo_texto", "adjunto": adjunto, "contenido": "hola" })).await;
        assert!(r.is_ok(), "{r:?}");

        // Guarda: lo que el jugador manda de más (otros dueños, única) lo corrige el host
        let mut nueva = borrador.clone();
        nueva.nombre = "Mi guerrera".into();
        nueva.es_unico = true;
        nueva.propietarios = vec![];
        nueva.archivos_adjuntos = vec![Some(ruta.clone())];
        let (_, r) = pedir(&mut ana, 4, json!({ "cmd": "crear_entidad", "entidad": serde_json::to_value(&nueva).unwrap() })).await;
        assert!(r.is_ok(), "{r:?}");
        let guardada = sala.estado().lock().banco_entidades.iter().find(|e| e.id == borrador.id).unwrap().clone();
        assert_eq!((guardada.nombre.as_str(), guardada.es_unico, guardada.propietarios.clone()), ("Mi guerrera", false, vec![id]));
        assert!(sala.red.borradores_de(id).is_empty());

        // Ya no es un borrador: no se puede crear otra vez, y como ficha suya sí la puede editar
        assert!(pedir(&mut ana, 5, json!({ "cmd": "crear_entidad", "entidad": serde_json::to_value(&nueva).unwrap() })).await.1.is_err());
        let mut editada = guardada.clone();
        editada.nombre = "Otra".into();
        assert!(pedir(&mut ana, 6, json!({ "cmd": "modificar_entidad", "entidad": serde_json::to_value(&editada).unwrap() })).await.1.is_ok());
    }

    #[tokio::test]
    async fn los_borradores_de_un_jugador_se_anulan_al_cancelar_y_al_cerrar_la_sala() {
        let sala = sala_con_app("srv_borradores").await;
        let (mut ana, id, _) = entrar(&sala, "Ana").await;
        let anulados = Arc::new(Mutex::new(Vec::new()));
        let guardados = anulados.clone();
        sala.red.poner_limpiador(move |ids| guardados.lock().unwrap().extend(ids));

        let iniciar = |n| json!({ "cmd": "iniciar_borrador_entidad", "zonaId": 1, "fotoUrl": null, "n": n });
        let mut ids = Vec::new();
        for n in 0..crate::red::MAX_BORRADORES_POR_JUGADOR {
            let (_, r) = pedir(&mut ana, n as u32, iniciar(n)).await;
            ids.push(serde_json::from_value::<Entidad>(r.unwrap()).unwrap().id);
        }
        assert!(pedir(&mut ana, 10, iniciar(9)).await.1.is_err(), "tope de fichas nuevas abiertas");

        // Cancelar una libera su id y su hueco
        let (_, r) = pedir(&mut ana, 11, json!({ "cmd": "cancelar_borrador_entidad", "id": ids[0], "rutas": [] })).await;
        assert!(r.is_ok(), "{r:?}");
        assert!(!sala.red.borradores_de(id).contains(&ids[0]));
        assert!(pedir(&mut ana, 12, json!({ "cmd": "cancelar_borrador_entidad", "id": 4000, "rutas": [] })).await.1.is_err(), "no es suyo");

        // Cerrar la sala anula los que quedan
        sala.red.cerrar();
        let mut anulados = anulados.lock().unwrap().clone();
        anulados.sort();
        assert_eq!(anulados, vec![ids[1], ids[2]]);
    }

    // ------------------------------------------------------------------
    // `atender` sobre un transporte en memoria (sin WebSocket)
    // ------------------------------------------------------------------

    async fn leer_mem(t: &mut Transporte) -> MensajeHost {
        loop {
            match tokio::time::timeout(Duration::from_secs(10), t.rx.recv()).await.expect("no llegó a tiempo").expect("cerrado") {
                Trama::Texto(s) | Trama::Efimero(s) => return serde_json::from_str(&s).unwrap(),
                Trama::Binario(_) => continue,
            }
        }
    }

    async fn mandar_mem(t: &Transporte, m: &MensajeCliente) {
        transporte::enviar_trama(&t.tx, Trama::Texto(serde_json::to_string(m).unwrap())).await.unwrap();
    }

    /// Entra un jugador por un transporte en memoria. Devuelve el extremo del jugador, ya con la vista leída.
    async fn entrar_mem(red: &Arc<Red>, nombre: &str) -> Transporte {
        let (host, mut j) = transporte::en_memoria();
        tokio::spawn(atender(red.clone(), host));
        let MensajeHost::Reto { reto } = leer_mem(&mut j).await else { panic!("primero va el reto") };
        mandar_mem(&j, &hola(red, &reto, None)).await;
        assert!(matches!(leer_mem(&mut j).await, MensajeHost::Identificate { .. }));
        mandar_mem(&j, &MensajeCliente::Unirse { nombre: nombre.into(), contrasena: None }).await;
        assert!(matches!(leer_mem(&mut j).await, MensajeHost::Bienvenido { .. }));
        assert!(matches!(leer_mem(&mut j).await, MensajeHost::Vista { .. }));
        j
    }

    #[tokio::test]
    async fn atender_funciona_igual_sobre_un_transporte_en_memoria() {
        let (red, _) = red_de_prueba("srv_memoria", None).await;
        let mut j = entrar_mem(&red, "Ana").await;
        assert_eq!(red.conectados(), vec![1]);
        // Una acción recibe su respuesta por el mismo transporte
        mandar_mem(&j, &MensajeCliente::Pedir { id: 1, peticion: serde_json::json!({ "cmd": "tirar_dado", "comando": "1d6" }) }).await;
        assert!(matches!(leer_mem(&mut j).await, MensajeHost::Respuesta { id: 1, .. }));
        // Al soltar el transporte del jugador, se le desconecta
        drop(j);
        for _ in 0..50 {
            if red.conectados().is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("seguía conectado");
    }

    #[tokio::test]
    async fn a_un_jugador_que_no_lee_se_le_corta_sin_acumular_mensajes() {
        let (red, _) = red_de_prueba("srv_no_lee", None).await;
        let j = entrar_mem(&red, "Ana").await;
        // Más respuestas de las que caben en su cola de salida, y no lee ninguna
        for id in 0..(transporte::CAPACIDAD as u32 + 10) {
            mandar_mem(&j, &MensajeCliente::Pedir { id, peticion: serde_json::json!({ "cmd": "tirar_dado", "comando": "1d6" }) }).await;
        }
        let t0 = std::time::Instant::now();
        while !red.conectados().is_empty() {
            assert!(t0.elapsed() < transporte::TIEMPO_ENVIO * 3, "debía cortarle al no poder enviarle nada");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    // ------------------------------------------------------------------
    // Imágenes por el protocolo (PedirImagen / SubirImagen)
    // ------------------------------------------------------------------

    /// Una sala con el mapa 1 compartido (fondo `imagenes/fondo.png`, de `tam` bytes) y el 2 sin compartir.
    async fn sala_con_fondo(nombre: &str, tam: usize) -> (Arc<Red>, Vec<u8>) {
        let (red, _) = red_de_prueba(nombre, None).await;
        let dir = red.dir_sala().unwrap();
        std::fs::create_dir_all(dir.join("imagenes")).unwrap();
        let fondo: Vec<u8> = (0..tam).map(|i| (i * 7 % 251) as u8).collect();
        std::fs::write(dir.join("imagenes/fondo.png"), &fondo).unwrap();
        std::fs::write(dir.join("imagenes/oculto.png"), b"PNG oculto").unwrap();
        {
            let mut est = red.estado_partida();
            let mapa = |id, ruta: &str, compartido| crate::tablero_general::modelos::Mapa {
                id, id_zona: 1, nombre: "M".into(), url_imagen_fondo: Some(ruta.into()), ancho: 1000, alto: 1000, rejilla: 50,
                color_opacidad: vec![255, 255, 255, 76], grosor: 1, orden: 0, compartido,
            };
            est.mapas = vec![mapa(1, "imagenes/fondo.png", true), mapa(2, "imagenes/oculto.png", false)];
        }
        (red, fondo)
    }

    type Imagen = Result<(String, Vec<u8>), String>;

    /// Recoge lo que llega para las imágenes `ids` (que pueden llegar intercaladas): bytes y tipo, o el error.
    async fn recibir_imagenes(j: &mut Transporte, ids: &[u32]) -> HashMap<u32, Imagen> {
        let mut listas: HashMap<u32, Imagen> = HashMap::new();
        let mut a_medias: HashMap<u32, (String, u64, Vec<u8>)> = HashMap::new();
        while listas.len() < ids.len() {
            let t = tokio::time::timeout(Duration::from_secs(10), j.rx.recv()).await.expect("no llegó a tiempo").expect("cerrado");
            match t {
                Trama::Texto(s) | Trama::Efimero(s) => match serde_json::from_str::<MensajeHost>(&s).unwrap() {
                    MensajeHost::ImagenCabecera { id, mime, tam } => {
                        a_medias.insert(id, (mime, tam, Vec::new()));
                    }
                    MensajeHost::ImagenError { id, error } => {
                        listas.insert(id, Err(error));
                    }
                    _ => {}
                },
                Trama::Binario(b) => {
                    let (tipo, id, trozo) = imagenes::leer_trozo(&b).unwrap();
                    assert_eq!(tipo, crate::red::protocolo::TROZO_BAJADA);
                    assert!(trozo.len() <= imagenes::TROZO, "trozos de 16 KiB como mucho");
                    let (_, _, datos) = a_medias.get_mut(&id).expect("los trozos llegan después de su cabecera");
                    datos.extend_from_slice(trozo);
                }
            }
            let terminadas: Vec<u32> = a_medias.iter().filter(|(_, (_, tam, d))| d.len() as u64 == *tam).map(|(id, _)| *id).collect();
            for id in terminadas {
                let (mime, _, datos) = a_medias.remove(&id).unwrap();
                listas.insert(id, Ok((mime, datos)));
            }
        }
        listas
    }

    async fn recibir_imagen(j: &mut Transporte, id: u32) -> Imagen {
        recibir_imagenes(j, &[id]).await.remove(&id).unwrap()
    }

    #[tokio::test]
    async fn una_imagen_llega_en_trozos_por_la_conexion_y_solo_si_la_ve() {
        let (red, fondo) = sala_con_fondo("srv_img_proto", 100 * 1024 + 7).await;
        let mut j = entrar_mem(&red, "Ana").await;
        let pedir = |id, ruta: &str| MensajeCliente::PedirImagen { id, ruta: ruta.into(), mini: None };

        mandar_mem(&j, &pedir(1, "imagenes/fondo.png")).await;
        let (mime, datos) = recibir_imagen(&mut j, 1).await.unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!(datos, fondo, "llega entera y en orden (7 trozos)");

        mandar_mem(&j, &pedir(2, "imagenes/oculto.png")).await;
        assert!(recibir_imagen(&mut j, 2).await.is_err(), "de un mapa sin compartir, no");
        mandar_mem(&j, &pedir(3, "imagenes/no_existe.png")).await;
        assert!(recibir_imagen(&mut j, 3).await.is_err());
        mandar_mem(&j, &pedir(4, "../../fuera.png")).await;
        assert!(recibir_imagen(&mut j, 4).await.is_err(), "ruta fuera de la partida");
    }

    #[tokio::test]
    async fn con_demasiadas_imagenes_pedidas_el_host_contesta_ocupado() {
        let (red, _) = sala_con_fondo("srv_img_ocupado", 100 * 1024).await;
        let mut j = entrar_mem(&red, "Ana").await;
        let total = (imagenes::MAX_BAJADAS_A_LA_VEZ + imagenes::MAX_BAJADAS_EN_COLA + 5) as u32;
        // Se piden todas sin leer nada: las primeras llenan la salida y el resto espera turno
        for id in 0..total {
            mandar_mem(&j, &MensajeCliente::PedirImagen { id, ruta: "imagenes/fondo.png".into(), mini: None }).await;
        }
        let ids: Vec<u32> = (0..total).collect();
        let mut ocupado = 0;
        let mut completas = 0;
        for (_, r) in recibir_imagenes(&mut j, &ids).await {
            match r {
                Ok(_) => completas += 1,
                Err(e) if e == imagenes::OCUPADO => ocupado += 1,
                Err(e) => panic!("{e}"),
            }
        }
        assert_eq!(ocupado, 5, "las que no caben en la cola");
        assert_eq!(completas, total - 5);
    }

    #[tokio::test]
    async fn un_jugador_sube_una_imagen_por_la_conexion_y_luego_la_puede_ver() {
        let (red, _) = sala_con_fondo("srv_img_subida", 10).await;
        let mut j = entrar_mem(&red, "Ana").await;
        let imagen = png();
        let subir_trozos = |id: u32, datos: &[u8]| -> Vec<Trama> {
            datos.chunks(7).map(|c| Trama::Binario(imagenes::trozo(crate::red::protocolo::TROZO_SUBIDA, id, c))).collect()
        };
        async fn esperar_subida(j: &mut Transporte, id: u32) -> Result<String, String> {
            loop {
                match leer_mem(j).await {
                    MensajeHost::SubidaOk { id: i, ruta } if i == id => return Ok(ruta),
                    MensajeHost::SubidaError { id: i, error } if i == id => return Err(error),
                    _ => {}
                }
            }
        }

        mandar_mem(&j, &MensajeCliente::SubirImagen { id: 1, ext: "png".into(), tam: imagen.len() }).await;
        for t in subir_trozos(1, &imagen) {
            transporte::enviar_trama(&j.tx, t).await.unwrap();
        }
        let ruta = esperar_subida(&mut j, 1).await.expect("se guarda");
        assert!(ruta.starts_with("imagenes/") && ruta.ends_with(".png"));
        // La ve quien la subió, aunque aún no la use ninguna ficha
        mandar_mem(&j, &MensajeCliente::PedirImagen { id: 9, ruta: ruta.clone(), mini: None }).await;
        assert_eq!(recibir_imagen(&mut j, 9).await.unwrap().1, imagen);

        // Más datos de los anunciados, o algo que no es una imagen: no
        mandar_mem(&j, &MensajeCliente::SubirImagen { id: 2, ext: "png".into(), tam: 3 }).await;
        transporte::enviar_trama(&j.tx, subir_trozos(2, b"demasiado")[0].clone()).await.unwrap();
        assert!(esperar_subida(&mut j, 2).await.is_err());
        mandar_mem(&j, &MensajeCliente::SubirImagen { id: 3, ext: "png".into(), tam: 9 }).await;
        transporte::enviar_trama(&j.tx, Trama::Binario(imagenes::trozo(crate::red::protocolo::TROZO_SUBIDA, 3, b"no es png"))).await.unwrap();
        assert!(esperar_subida(&mut j, 3).await.is_err());
        mandar_mem(&j, &MensajeCliente::SubirImagen { id: 4, ext: "png".into(), tam: crate::tablero_general::imagenes::MAX_BYTES_SUBIDA + 1 }).await;
        assert!(esperar_subida(&mut j, 4).await.is_err(), "demasiado grande: ni empieza");
    }

    #[tokio::test]
    async fn el_servidor_de_la_sala_sirve_la_pagina_de_los_jugadores() {
        let (_red, puerto) = red_de_prueba("srv_pagina", None).await;
        let url = |camino: &str| format!("http://127.0.0.1:{puerto}{camino}");
        let r = reqwest::get(url("/")).await.unwrap();
        if !pagina::incluida() {
            // Sin `bun run web:build:app`: lo dice en vez de fallar en silencio
            assert_eq!(r.status(), 503);
            return;
        }
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers()["content-type"], "text/html; charset=utf-8");
        assert_eq!(r.headers()["cache-control"], "no-cache", "index.html siempre se revisa (nada de páginas viejas)");
        assert_eq!(r.headers()["x-frame-options"], "DENY");
        assert_eq!(r.headers()["x-content-type-options"], "nosniff");
        let html = r.text().await.unwrap();
        // Un archivo de `_app/immutable/` (con hash en el nombre) se guarda para siempre
        let inicio = html.find("/_app/immutable/").expect("la página carga sus piezas de /_app/immutable/");
        let fin = inicio + html[inicio..].find('"').unwrap();
        let r = reqwest::get(url(&html[inicio..fin])).await.unwrap();
        assert_eq!(r.status(), 200);
        assert!(r.headers()["cache-control"].to_str().unwrap().contains("immutable"));
        assert_eq!(reqwest::get(url("/no/existe.js")).await.unwrap().status(), 404);
        assert_eq!(reqwest::get(url("/../../Cargo.toml")).await.unwrap().status(), 404, "nada fuera de la página");
        // `/ws` y `/imagen` siguen siendo lo que eran
        assert_eq!(reqwest::get(url("/imagen?ruta=imagenes%2Fx.png&sesion=nada")).await.unwrap().status(), 403);
    }

    #[tokio::test]
    async fn en_red_local_se_entra_sin_llave_y_en_los_demas_modos_no() {
        let (red, puerto) = red_de_prueba("srv_sin_llave", None).await;
        let sin_llave = MensajeCliente::Hola { version: VERSION_PROTOCOLO, sala: "".into(), secreto: "".into(), prueba: None };

        // Modo normal (túnel o dirección propia): sin la llave del enlace, no se pasa del saludo
        let (mut c, _) = conectar_con_reto(puerto, &origen(&red)).await;
        mandar(&mut c, &sin_llave).await;
        assert!(matches!(leer(&mut c).await, MensajeHost::Rechazado { .. }));

        // En red local (como «abrir en LAN»): la confianza está en la red, basta con llegar y dar el nombre
        red.poner_respaldo_para_tests(crate::red::config::Respaldo::Lan { puerto });
        let (mut c, _) = conectar_con_reto(puerto, &format!("http://localhost:{puerto}")).await;
        mandar(&mut c, &sin_llave).await;
        assert!(matches!(leer(&mut c).await, MensajeHost::Identificate { .. }), "entra sin llave");
        mandar(&mut c, &MensajeCliente::Unirse { nombre: "Ana".into(), contrasena: None }).await;
        assert!(matches!(leer(&mut c).await, MensajeHost::Bienvenido { .. }));
    }

    #[tokio::test]
    async fn en_red_local_la_contrasena_sigue_valiendo_si_se_pone() {
        let (red, puerto) = red_de_prueba("srv_lan_contrasena", Some("dragón")).await;
        red.poner_respaldo_para_tests(crate::red::config::Respaldo::Lan { puerto });
        let (mut c, _) = conectar_con_reto(puerto, &format!("http://localhost:{puerto}")).await;
        mandar(&mut c, &MensajeCliente::Hola { version: VERSION_PROTOCOLO, sala: "".into(), secreto: "".into(), prueba: None }).await;
        assert_eq!(leer(&mut c).await, MensajeHost::Identificate { pide_contrasena: true, error: None });
        mandar(&mut c, &MensajeCliente::Unirse { nombre: "Ana".into(), contrasena: Some("otra".into()) }).await;
        assert!(matches!(leer(&mut c).await, MensajeHost::Identificate { error: Some(_), .. }), "sin la contraseña no entra");
    }
}
