// src-tauri/src/red/mod.rs
//
// La sala: el host abre un servidor propio (servidor.rs) en 127.0.0.1, un túnel (tunel.rs) lo publica en
// una dirección https temporal y los jugadores entran desde el navegador con el enlace, que abre el
// cliente web publicado en una dirección fija (`URL_CLIENTE`, carpeta cliente-web/). Los datos solo van
// host → jugador y jugador → host, nunca de un jugador a otro.
//
// Reglas:
//  * Lo único que sale hacia un jugador es su vista (vista.rs), calculada con los filtros de visibilidad.rs.
//  * Orden de bloqueo: SIEMPRE el estado de la partida primero y `Red::dentro` después. `tras_cambio` se
//    llama desde `partida::registrar`, con el estado cogido; las tareas del servidor nunca cogen el estado
//    teniendo `dentro`.
//  * La identidad de la sala (enlace, contraseña, jugadores recordados) está en `sala.json`, que es solo del
//    host: no pasa por el estado ni por los deltas.
pub mod comandos;
pub mod codigo;
pub mod config;
pub mod pagina;
pub mod imagenes;
pub mod peticiones;
pub mod protocolo;
pub mod sala;
pub mod servidor;
pub mod transporte;
pub mod trozos;
pub mod webrtc;
pub mod tunel;
pub mod vista;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::{mpsc, oneshot};

use crate::red::peticiones::Peticion;
use crate::red::protocolo::MensajeHost;
use crate::red::sala::SalaGuardada;
use crate::red::vista::{VistaJugador, diferencias, vista_para};
use crate::tablero_general::almacen::{Almacen, bloquear};
use crate::tablero_general::estado::EstadoPartida;
use crate::tablero_general::modelos::IdJugador;
use crate::tablero_general::tablero::AvisoTablero;

/// Dónde está publicado el cliente web (cliente-web/, GitHub Pages). Para probar en local se compila con
/// `OPENROLL_URL_CLIENTE=http://localhost:5173/` y se lanza `bun run web:dev`.
pub const URL_CLIENTE: &str = match option_env!("OPENROLL_URL_CLIENTE") {
    Some(url) => url,
    None => "https://kiro55-dev.github.io/vtt-rol/",
};

/// Fallos de contraseña permitidos por conexión antes de echarla, y en total por minuto.
const FALLOS_POR_CONEXION: u32 = 5;
const FALLOS_POR_MINUTO: usize = 20;
/// Conexiones que pueden estar a la vez saludando o escribiendo su nombre (sin ser aún nadie).
pub(crate) const MAX_SIN_IDENTIFICAR: usize = 16;

/// En qué punto está la sala (lo enseña la ventana «Configuración de sala»).
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "tipo")]
pub enum Fase {
    Cerrada,
    /// Abriéndose: `paso` dice qué está haciendo y `progreso` (0..1), si se sabe, cuánto lleva.
    Preparando { paso: String, progreso: Option<f32> },
    Abierta,
}

/// Lo que el front necesita para pintar la ventana de la sala.
#[derive(Serialize, Clone, Debug)]
pub struct EstadoSala {
    pub fase: Fase,
    pub enlace: Option<String>,
    pub pide_contrasena: bool,
    pub conectados: Vec<IdJugador>,
    /// Los mapas que ven los jugadores (de todas las zonas), para la ventana de la sala.
    pub mapas_compartidos: Vec<MapaCompartido>,
    /// Por qué falló lo último (abrir la sala...), hasta que se vuelva a intentar.
    pub error: Option<String>,
    /// Cómo se publica: el que usa la sala abierta o, cerrada, el que usará al abrirse (`red.json`).
    pub respaldo: config::Respaldo,
    /// Todos los enlaces de la sala abierta (en red local, uno por cada IP del equipo). `enlace` es el primero.
    pub enlaces: Vec<EnlaceSala>,
    /// Si la app lleva dentro la página de los jugadores (hace falta para la dirección propia y la red local).
    pub pagina_incluida: bool,
}

/// Un enlace para los jugadores y de qué red es (el nombre de la interfaz en red local, p. ej. «Radmin VPN»).
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct EnlaceSala {
    pub etiqueta: String,
    pub url: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct MapaCompartido {
    pub id: u16,
    pub nombre: String,
}

/// Un jugador conectado: por dónde se le envía y qué vio la última vez (para mandarle solo lo que cambia).
struct Conexion {
    id_conexion: u64,
    enviar: mpsc::UnboundedSender<MensajeHost>,
    ultima: VistaJugador,
}

struct SalaAbierta {
    sala: SalaGuardada,
    /// Carpeta de la partida de esta sala (la del `Almacen` al abrirla; no puede cambiar mientras está abierta).
    dir: PathBuf,
    fase: Fase,
    /// Dirección pública del túnel (`xxxx.trycloudflare.com`), cuando ya la hay.
    tunel: Option<String>,
    conexiones: HashMap<IdJugador, Conexion>,
    /// Sesión de imágenes de cada jugador (`/imagen?sesion=…`). Se conserva al reconectar, para que las URLs
    /// que ya tiene su navegador sigan valiendo; se olvida al expulsarlo, al cambiar el enlace y al cerrar.
    sesiones: HashMap<IdJugador, String>,
    /// Fallos de contraseña del último minuto, de cualquier conexión.
    fallos: VecDeque<Instant>,
    apagar_servidor: Option<oneshot::Sender<()>>,
    proceso_tunel: Option<tokio::process::Child>,
    /// Imágenes que ha subido cada jugador en esta sala (rutas relativas): solo él las puede ver y usar en
    /// su ficha, y las puede descartar si no las usa. Con un tope por jugador.
    subidas: HashMap<IdJugador, HashSet<String>>,
    /// Para que una apertura que se canceló (se cerró la sala a mitad) no pise a la siguiente.
    generacion: u64,
    /// Cómo se publicó al abrirla (no cambia mientras está abierta).
    respaldo: config::Respaldo,
    /// En red local: las IPs de este equipo al abrir la sala (cada una con su enlace).
    lan: Vec<config::DireccionLan>,
    /// Códigos de la conexión directa ya usados (su `n`): no se aceptan dos veces. Los últimos 256.
    codigos_vistos: VecDeque<String>,
}

impl SalaAbierta {
    /// El fragmento del enlace (tras el `#`) con la identidad de la sala.
    fn fragmento(&self) -> String {
        match self.huella() {
            Some(f) => format!("s={}&k={}&f={f}", self.sala.id, self.sala.secreto),
            None => format!("s={}&k={}", self.sala.id, self.sala.secreto),
        }
    }

    /// Peer to peer (con Cloudflare o dirección propia de respaldo); en red local no hay conexión directa.
    fn p2p(&self) -> bool {
        !matches!(self.respaldo, config::Respaldo::Lan { .. })
    }

    /// La huella del certificado de la conexión directa, si la sala va en peer to peer.
    fn huella(&self) -> Option<String> {
        if self.p2p() { self.sala.huella() } else { None }
    }

    /// Los enlaces para los jugadores. Con el túnel de Cloudflare, a la página de GitHub Pages con `t`; con la
    /// dirección propia o en red local, a la propia dirección (la página la sirve la app), sin `t`.
    fn enlaces(&self) -> Vec<EnlaceSala> {
        match &self.respaldo {
            config::Respaldo::Cloudflare => self
                .tunel
                .iter()
                .map(|t| EnlaceSala { etiqueta: "Túnel de Cloudflare".into(), url: enlace(URL_CLIENTE, t, &self.sala) })
                .collect(),
            config::Respaldo::Propio { .. } => self
                .tunel
                .iter()
                .map(|d| EnlaceSala { etiqueta: "Dirección propia".into(), url: format!("https://{d}/#{}", self.fragmento()) })
                .collect(),
            // Como «abrir en LAN» de un juego: el enlace es solo `http://<ip>:<puerto>/`, sin llave
            config::Respaldo::Lan { puerto } => self
                .lan
                .iter()
                .map(|d| EnlaceSala { etiqueta: d.interfaz.clone(), url: format!("http://{}:{puerto}/", d.ip) })
                .collect(),
        }
    }

    fn enlace(&self) -> Option<String> {
        self.enlaces().into_iter().next().map(|e| e.url)
    }

    /// Si entrar NO exige la llave del enlace (`s` y `k`). Solo en red local: ahí la confianza está en la red, como
    /// en «abrir en LAN» de un juego (y la contraseña sigue siendo opcional). Con el túnel o la dirección propia, que
    /// están en internet, el enlace sin llave no vale.
    fn sin_llave(&self) -> bool {
        matches!(self.respaldo, config::Respaldo::Lan { .. })
    }

    /// Las direcciones por las que los jugadores llegan a ESTA sala (`host` o `host:puerto`): a ellas va atada la
    /// prueba del token (`sala::prueba`). Solo las que pone el host, nunca lo que diga una cabecera de la petición.
    fn direcciones(&self) -> Vec<String> {
        match &self.respaldo {
            config::Respaldo::Lan { puerto } => {
                let mut v: Vec<String> = self.lan.iter().map(|d| format!("{}:{puerto}", d.ip)).collect();
                v.push(format!("localhost:{puerto}"));
                v.push(format!("127.0.0.1:{puerto}"));
                v
            }
            _ => {
                let mut v: Vec<String> = self.tunel.iter().cloned().collect();
                // Por la conexión directa, la prueba del token va atada a la huella del host
                if let Some(f) = self.huella() {
                    v.push(format!("webrtc:{f}"));
                }
                v
            }
        }
    }

    /// Los orígenes de la página cuando la sirve la propia sala (el de GitHub Pages está en `Red::origenes`).
    fn origenes_propios(&self) -> Vec<String> {
        let esquema = if matches!(self.respaldo, config::Respaldo::Lan { .. }) { "http" } else { "https" };
        self.direcciones().into_iter().filter(|d| !d.starts_with("webrtc:")).map(|d| format!("{esquema}://{d}")).collect()
    }
}

/// El enlace que se pasa a los jugadores. Todo va tras el `#`: el navegador nunca lo envía a ningún servidor.
pub fn enlace(url_cliente: &str, tunel: &str, sala: &SalaGuardada) -> String {
    format!("{url_cliente}#t={tunel}&s={}&k={}", sala.id, sala.secreto)
}

pub struct Red {
    estado: Arc<Mutex<EstadoPartida>>,
    almacen: Arc<Almacen>,
    /// Carpeta de datos de la app (cloudflared se guarda en `<raiz>/bin/`).
    raiz: PathBuf,
    dentro: Mutex<Option<SalaAbierta>>,
    ultimo_error: Mutex<Option<String>>,
    /// Avisa al front de que algo cambió (evento `sala-cambiada`). Lo pone lib.rs.
    al_cambiar: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
    /// Orígenes desde los que se acepta el WebSocket (el del cliente web y, en desarrollo, localhost).
    pub(crate) origenes: Vec<String>,
    siguiente_id: AtomicU64,
    /// Emite un evento al front del host (`partida-cambiada`, `aviso-tablero`). Lo pone lib.rs.
    emisor: Mutex<Option<Box<dyn Fn(&str, serde_json::Value) + Send + Sync>>>,
    /// Ejecuta lo que pide un jugador (peticiones::ejecutar con el `AppHandle`). Lo pone lib.rs.
    ejecutor: Mutex<Option<Ejecutor>>,
    /// Fichas nuevas (borradores con id reservado) que tiene abiertas cada jugador. Si el jugador se va (se cierra la
    /// sala, se le expulsa o cambia el enlace) se anulan con `limpiador`, para no dejar ids reservados.
    borradores: Mutex<HashMap<IdJugador, HashSet<u16>>>,
    limpiador: Mutex<Option<Limpiador>>,
    /// Conexiones que aún no son ningún jugador (ver `MAX_SIN_IDENTIFICAR`).
    sin_identificar: std::sync::atomic::AtomicUsize,
    /// Conexiones directas (WebRTC) a medio hacer: ofertas contestadas cuyos canales aún no se han abierto.
    peers_a_medias: std::sync::atomic::AtomicUsize,
}

/// Una plaza de conexión sin identificar; se libera sola al soltarla.
pub(crate) struct PlazaSaludo<'a>(&'a Red);

impl Drop for PlazaSaludo<'_> {
    fn drop(&mut self) {
        self.0.sin_identificar.fetch_sub(1, Ordering::Relaxed);
    }
}

type Limpiador = Arc<dyn Fn(Vec<u16>) + Send + Sync>;

/// Borradores que puede tener abiertos un jugador a la vez.
pub const MAX_BORRADORES_POR_JUGADOR: usize = 3;

type Ejecutor = Arc<dyn Fn(IdJugador, Peticion) -> Result<serde_json::Value, String> + Send + Sync>;

/// Imágenes que puede subir un jugador en una sala.
const MAX_SUBIDAS_POR_JUGADOR: usize = 20;

/// La `Red` de la aplicación, para el enganche de `partida::registrar` (que no tiene el `AppState` a mano).
/// En los tests no se instala: `tras_cambio` no hace nada.
static RED: OnceLock<Arc<Red>> = OnceLock::new();

pub fn instalar(red: Arc<Red>) {
    let _ = RED.set(red);
}

/// Tras cada delta (con el estado cogido): manda a cada jugador conectado lo que ha cambiado en su vista.
pub fn tras_cambio(est: &EstadoPartida) {
    if let Some(red) = RED.get() {
        red.tras_cambio(est);
    }
}

/// Origen (`esquema://host[:puerto]`) de una URL.
fn origen(url: &str) -> String {
    let sin_esquema = url.split_once("://").map(|(e, r)| (e, r));
    match sin_esquema {
        Some((esquema, resto)) => format!("{esquema}://{}", resto.split('/').next().unwrap_or("")),
        None => url.to_string(),
    }
}

impl Red {
    pub fn nueva(estado: Arc<Mutex<EstadoPartida>>, almacen: Arc<Almacen>, raiz: PathBuf) -> Self {
        let mut origenes = vec![origen(URL_CLIENTE)];
        if cfg!(debug_assertions) {
            origenes.extend(["http://localhost:5173", "http://127.0.0.1:5173"].map(String::from));
        }
        Red {
            estado,
            almacen,
            raiz,
            dentro: Mutex::new(None),
            ultimo_error: Mutex::new(None),
            al_cambiar: Mutex::new(None),
            origenes,
            siguiente_id: AtomicU64::new(1),
            emisor: Mutex::new(None),
            ejecutor: Mutex::new(None),
            borradores: Mutex::new(HashMap::new()),
            limpiador: Mutex::new(None),
            sin_identificar: std::sync::atomic::AtomicUsize::new(0),
            peers_a_medias: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Ocupa una plaza para saludar. `None` si ya hay demasiadas conexiones sin identificar.
    pub(crate) fn plaza_saludo(&self) -> Option<PlazaSaludo<'_>> {
        let antes = self.sin_identificar.fetch_add(1, Ordering::Relaxed);
        let plaza = PlazaSaludo(self);
        (antes < MAX_SIN_IDENTIFICAR).then_some(plaza)
    }

    pub fn poner_limpiador(&self, f: impl Fn(Vec<u16>) + Send + Sync + 'static) {
        *bloquear(&self.limpiador) = Some(Arc::new(f));
    }

    /// Los borradores abiertos de `jugador`.
    pub(crate) fn borradores_de(&self, jugador: IdJugador) -> HashSet<u16> {
        bloquear(&self.borradores).get(&jugador).cloned().unwrap_or_default()
    }

    /// Anota un borrador del jugador. `Err` si ya tiene demasiados abiertos.
    pub(crate) fn anotar_borrador(&self, jugador: IdJugador, id: u16) -> Result<(), String> {
        let mut b = bloquear(&self.borradores);
        let suyos = b.entry(jugador).or_default();
        if suyos.len() >= MAX_BORRADORES_POR_JUGADOR {
            return Err("Tienes demasiadas fichas nuevas abiertas".into());
        }
        suyos.insert(id);
        Ok(())
    }

    pub(crate) fn soltar_borrador(&self, jugador: IdJugador, id: u16) {
        if let Some(s) = bloquear(&self.borradores).get_mut(&jugador) {
            s.remove(&id);
        }
    }

    /// Anula los borradores de un jugador (`None`: de todos). Sin ningún cerrojo cogido al llamar al limpiador.
    fn anular_borradores(&self, jugador: Option<IdJugador>) {
        let ids: Vec<u16> = {
            let mut b = bloquear(&self.borradores);
            match jugador {
                Some(j) => b.remove(&j).map(|s| s.into_iter().collect()).unwrap_or_default(),
                None => b.drain().flat_map(|(_, s)| s).collect(),
            }
        };
        if ids.is_empty() {
            return;
        }
        let limpiador = bloquear(&self.limpiador).clone();
        if let Some(f) = limpiador {
            f(ids);
        }
    }

    pub fn poner_emisor(&self, f: impl Fn(&str, serde_json::Value) + Send + Sync + 'static) {
        *bloquear(&self.emisor) = Some(Box::new(f));
    }

    pub fn poner_ejecutor(&self, f: impl Fn(IdJugador, Peticion) -> Result<serde_json::Value, String> + Send + Sync + 'static) {
        *bloquear(&self.ejecutor) = Some(Arc::new(f));
    }

    /// Un evento para la pantalla del host (relee o pinta lo que llega de un jugador).
    pub(crate) fn emitir(&self, evento: &str, payload: serde_json::Value) {
        if let Some(f) = bloquear(&self.emisor).as_ref() {
            f(evento, payload);
        }
    }

    /// Hace lo que pide un jugador (ya decodificado), o dice por qué no. Bloqueante: llamar desde `spawn_blocking`.
    pub(crate) fn ejecutar_peticion(&self, jugador: IdJugador, p: Peticion) -> Result<serde_json::Value, String> {
        let ejecutor = bloquear(&self.ejecutor).clone().ok_or("El host no está listo")?;
        ejecutor(jugador, p)
    }

    pub fn al_cambiar(&self, f: impl Fn() + Send + Sync + 'static) {
        *bloquear(&self.al_cambiar) = Some(Box::new(f));
    }

    pub(crate) fn avisar(&self) {
        if let Some(f) = bloquear(&self.al_cambiar).as_ref() {
            f();
        }
    }

    fn dentro(&self) -> MutexGuard<'_, Option<SalaAbierta>> {
        bloquear(&self.dentro)
    }

    fn estado_partida(&self) -> MutexGuard<'_, EstadoPartida> {
        bloquear(&self.estado)
    }

    pub fn esta_abierta(&self) -> bool {
        self.dentro().is_some()
    }

    /// Lo que enseña la ventana de la sala. Cerrada, lee `sala.json` para saber si hay contraseña. Se llama con
    /// el estado de la partida (`est`) ya cogido: estado → red.
    pub fn estado(&self, est: &EstadoPartida) -> EstadoSala {
        let error = bloquear(&self.ultimo_error).clone();
        let mapas_compartidos =
            est.mapas.iter().filter(|m| m.compartido).map(|m| MapaCompartido { id: m.id, nombre: m.nombre.clone() }).collect();
        match self.dentro().as_ref() {
            Some(s) => EstadoSala {
                fase: s.fase.clone(),
                enlace: if matches!(s.fase, Fase::Abierta) { s.enlace() } else { None },
                pide_contrasena: s.sala.pide_contrasena(),
                conectados: s.conexiones.keys().copied().collect(),
                mapas_compartidos,
                error,
                respaldo: s.respaldo.clone(),
                enlaces: if matches!(s.fase, Fase::Abierta) { s.enlaces() } else { Vec::new() },
                pagina_incluida: pagina::incluida(),
            },
            None => EstadoSala {
                fase: Fase::Cerrada,
                enlace: None,
                pide_contrasena: SalaGuardada::leer_o_crear(&self.almacen.dir()).is_ok_and(|s| s.pide_contrasena()),
                conectados: Vec::new(),
                mapas_compartidos,
                error,
                respaldo: config::leer(&self.raiz).respaldo,
                enlaces: Vec::new(),
                pagina_incluida: pagina::incluida(),
            },
        }
    }

    // ------------------------------------------------------------------
    // Abrir y cerrar
    // ------------------------------------------------------------------

    /// La configuración de red de este equipo (`red.json`).
    pub fn config(&self) -> config::ConfigRed {
        config::leer(&self.raiz)
    }

    /// Cambia la configuración de red. Solo con la sala cerrada: la abierta sigue con la que tenía.
    pub fn guardar_config(&self, c: &config::ConfigRed) -> Result<(), String> {
        if self.esta_abierta() {
            return Err("Cierra la sala para cambiar cómo se publica".into());
        }
        config::guardar(&self.raiz, c)?;
        self.avisar();
        Ok(())
    }

    /// Abre la sala: servidor local y, según `red.json`, túnel de Cloudflare (cloudflared se descarga la primera
    /// vez) o la dirección propia del host. Avisa al front en cada paso. Si se cierra a mitad, lo que se estaba
    /// preparando se descarta.
    pub async fn abrir(self: &Arc<Self>) -> Result<(), String> {
        let generacion = self.siguiente_id.fetch_add(1, Ordering::Relaxed);
        let respaldo = config::leer(&self.raiz).respaldo;
        {
            let mut dentro = self.dentro();
            if dentro.is_some() {
                return Ok(());
            }
            let dir = self.almacen.dir();
            let mut sala = SalaGuardada::leer_o_crear(&dir)?;
            // Peer to peer: la identidad de la conexión directa (su huella va en el enlace)
            if !matches!(respaldo, config::Respaldo::Lan { .. }) && sala.asegurar_certificado()? {
                sala.guardar(&dir)?;
            }
            *dentro = Some(SalaAbierta {
                sala,
                dir,
                fase: Fase::Preparando { paso: "Arrancando el servidor…".into(), progreso: None },
                tunel: None,
                conexiones: HashMap::new(),
                sesiones: HashMap::new(),
                subidas: HashMap::new(),
                fallos: VecDeque::new(),
                apagar_servidor: None,
                proceso_tunel: None,
                generacion,
                respaldo: respaldo.clone(),
                lan: Vec::new(),
                codigos_vistos: VecDeque::new(),
            });
        }
        *bloquear(&self.ultimo_error) = None;
        self.avisar();

        let resultado = self.preparar(generacion, respaldo).await;
        if let Err(e) = &resultado {
            // Solo si sigue siendo esta apertura (no se cerró ni se volvió a abrir entretanto)
            if self.dentro().as_ref().is_some_and(|s| s.generacion == generacion) {
                self.cerrar();
                *bloquear(&self.ultimo_error) = Some(e.clone());
                self.avisar();
            }
        }
        resultado
    }

    /// Pasos de `abrir`. Tras cada espera comprueba que la sala sigue siendo la de esta apertura.
    async fn preparar(self: &Arc<Self>, generacion: u64, respaldo: config::Respaldo) -> Result<(), String> {
        // Dónde escucha: en red local, en todas las interfaces; si no, solo en este equipo (lo publica el túnel)
        let (ip, puerto_fijo) = match &respaldo {
            config::Respaldo::Propio { puerto, .. } => (std::net::Ipv4Addr::LOCALHOST, *puerto),
            config::Respaldo::Lan { puerto } => (std::net::Ipv4Addr::UNSPECIFIED, *puerto),
            config::Respaldo::Cloudflare => (std::net::Ipv4Addr::LOCALHOST, 0), // uno libre cualquiera
        };
        // (En los tests no hace falta haber construido la página.)
        if respaldo.sirve_pagina() && !pagina::incluida() && !cfg!(test) {
            return Err("Esta versión de la app no lleva la página de los jugadores (bun run web:build:app). Usa el túnel de Cloudflare.".into());
        }
        let (puerto, apagar) = servidor::arrancar(self.clone(), ip, puerto_fijo).await?;
        if !self.con_sala(generacion, |s| s.apagar_servidor = Some(apagar)) {
            return Ok(());
        }

        // Red local: los enlaces son las IPs de este equipo
        if let config::Respaldo::Lan { .. } = &respaldo {
            let lan = config::direcciones_lan();
            self.con_sala(generacion, |s| {
                s.lan = lan;
                s.fase = Fase::Abierta;
            });
            self.avisar();
            return Ok(());
        }

        // Dirección propia: la pone el host (su dominio, su túnel...); aquí solo se escucha en su puerto
        if let config::Respaldo::Propio { url, .. } = &respaldo {
            let direccion = config::direccion_de_url(url)?;
            self.con_sala(generacion, |s| {
                s.tunel = Some(direccion);
                s.fase = Fase::Abierta;
            });
            self.avisar();
            return Ok(());
        }

        let progreso = {
            let red = self.clone();
            move |hecho: u64, total: Option<u64>| {
                let fraccion = total.filter(|t| *t > 0).map(|t| hecho as f32 / t as f32);
                red.con_sala(generacion, |s| {
                    s.fase = Fase::Preparando { paso: "Descargando cloudflared (solo la primera vez)…".into(), progreso: fraccion }
                });
                red.avisar();
            }
        };
        let binario = tunel::asegurar_cloudflared(&self.raiz, progreso).await?;
        if !self.con_sala(generacion, |s| s.fase = Fase::Preparando { paso: "Abriendo el túnel…".into(), progreso: None }) {
            return Ok(());
        }
        self.avisar();

        let (proceso, direccion) = tunel::lanzar(&binario, puerto).await?;
        let mut proceso = Some(proceso);
        self.con_sala(generacion, |s| {
            s.proceso_tunel = proceso.take();
            s.tunel = Some(direccion);
            s.fase = Fase::Abierta;
        });
        // Si se cerró mientras tanto, `proceso` sigue aquí y se mata al soltarlo (kill_on_drop)
        drop(proceso);
        self.avisar();
        Ok(())
    }

    /// Cambia la sala si sigue siendo la de `generacion`. Devuelve si lo era.
    fn con_sala(&self, generacion: u64, f: impl FnOnce(&mut SalaAbierta)) -> bool {
        match self.dentro().as_mut() {
            Some(s) if s.generacion == generacion => {
                f(s);
                true
            }
            _ => false,
        }
    }

    /// Cierra la sala: avisa a los jugadores, para el servidor y el túnel.
    pub fn cerrar(&self) {
        let Some(mut s) = self.dentro().take() else { return };
        for c in s.conexiones.values() {
            let _ = c.enviar.send(MensajeHost::SalaCerrada);
        }
        if let Some(apagar) = s.apagar_servidor.take() {
            let _ = apagar.send(());
        }
        if let Some(mut p) = s.proceso_tunel.take() {
            let _ = p.start_kill();
        }
        let subidas: Vec<String> = s.subidas.drain().flat_map(|(_, r)| r).collect();
        let dir = s.dir.clone();
        drop(s);
        self.anular_borradores(None);
        self.descartar_subidas(&dir, subidas);
        self.avisar();
    }

    /// Borra del disco las imágenes que subieron los jugadores y no usa nada de la partida (una foto subida y
    /// luego no guardada en ninguna ficha). Sin `dentro` cogido: coge el estado (orden estado → red).
    fn descartar_subidas(&self, dir: &std::path::Path, rutas: Vec<String>) {
        if rutas.is_empty() {
            return;
        }
        let est = self.estado_partida();
        crate::tablero_general::recursos::descartar(dir, &est, &rutas);
    }

    // ------------------------------------------------------------------
    // Enlace, contraseña y jugadores
    // ------------------------------------------------------------------

    /// Cambia la sala de `sala.json` (la abierta, o la de la partida actual si está cerrada) y la guarda.
    fn cambiar_sala<R>(&self, f: impl FnOnce(&mut SalaGuardada) -> Result<R, String>) -> Result<R, String> {
        let r = {
            let mut dentro = self.dentro();
            match dentro.as_mut() {
                Some(s) => {
                    let r = f(&mut s.sala)?;
                    s.sala.guardar(&s.dir)?;
                    r
                }
                None => {
                    let dir = self.almacen.dir();
                    let mut sala = SalaGuardada::leer_o_crear(&dir)?;
                    let r = f(&mut sala)?;
                    sala.guardar(&dir)?;
                    r
                }
            }
        };
        self.avisar();
        Ok(r)
    }

    /// Enlace nuevo: el anterior deja de valer, se echa a los conectados y todos repiten la contraseña.
    pub fn regenerar_enlace(&self) -> Result<(), String> {
        self.cambiar_sala(|s| {
            s.regenerar();
            Ok(())
        })?;
        let mut subidas = (PathBuf::new(), Vec::new());
        if let Some(s) = self.dentro().as_mut() {
            s.sesiones.clear();
            subidas = (s.dir.clone(), s.subidas.drain().flat_map(|(_, r)| r).collect());
            for (_, c) in s.conexiones.drain() {
                let _ = c.enviar.send(MensajeHost::Rechazado { motivo: "El host ha cambiado el enlace de la sala".into() });
            }
        }
        self.anular_borradores(None);
        self.descartar_subidas(&subidas.0, subidas.1);
        self.avisar();
        Ok(())
    }

    pub fn poner_contrasena(&self, contrasena: Option<String>) -> Result<(), String> {
        self.cambiar_sala(|s| s.poner_contrasena(contrasena.as_deref()))
    }

    /// Echa a un jugador y olvida su navegador. Sigue registrado en la partida, con sus fichas.
    pub fn expulsar(&self, id: IdJugador) -> Result<(), String> {
        self.cambiar_sala(|s| {
            s.revocar(id);
            Ok(())
        })?;
        let mut subidas = (PathBuf::new(), Vec::new());
        if let Some(c) = self.dentro().as_mut().and_then(|s| {
            s.sesiones.remove(&id);
            subidas = (s.dir.clone(), s.subidas.remove(&id).unwrap_or_default().into_iter().collect());
            s.conexiones.remove(&id)
        }) {
            let _ = c.enviar.send(MensajeHost::Rechazado { motivo: "El host te ha sacado de la sala".into() });
        }
        self.anular_borradores(Some(id));
        self.descartar_subidas(&subidas.0, subidas.1);
        self.avisar();
        Ok(())
    }

    pub fn conectados(&self) -> Vec<IdJugador> {
        self.dentro().as_ref().map(|s| s.conexiones.keys().copied().collect()).unwrap_or_default()
    }

    // ------------------------------------------------------------------
    // Envíos
    // ------------------------------------------------------------------

    /// Tras un delta (con el estado ya cogido): a cada conectado, lo que cambió en su vista.
    pub fn tras_cambio(&self, est: &EstadoPartida) {
        let mut dentro = self.dentro();
        let Some(s) = dentro.as_mut() else { return };
        for (id, c) in s.conexiones.iter_mut() {
            let nueva = vista_para(*id, est);
            if let Some(cambios) = diferencias(&c.ultima, &nueva) {
                let _ = c.enviar.send(MensajeHost::Cambios { cambios });
                c.ultima = nueva;
            }
        }
    }

    /// Ping o recorrido de un jugador: lo ven el host y los demás jugadores (no él, que ya lo dibujó).
    pub(crate) fn reenviar_aviso(&self, de: IdJugador, aviso: &AvisoTablero) {
        if let Some(s) = self.dentro().as_ref() {
            for (_, c) in s.conexiones.iter().filter(|(id, _)| **id != de) {
                let _ = c.enviar.send(MensajeHost::Aviso { aviso: aviso.clone() });
            }
        }
        self.emitir("aviso-tablero", serde_json::to_value(aviso).unwrap_or_default());
    }

    /// Lo que ha subido `jugador` en esta sala.
    pub(crate) fn subidas_de(&self, jugador: IdJugador) -> HashSet<String> {
        self.dentro().as_ref().and_then(|s| s.subidas.get(&jugador).cloned()).unwrap_or_default()
    }

    /// Anota una imagen subida. `Err` si ya subió demasiadas.
    pub(crate) fn anotar_subida(&self, jugador: IdJugador, ruta: String) -> Result<(), String> {
        let mut dentro = self.dentro();
        let s = dentro.as_mut().ok_or("La sala se ha cerrado")?;
        let suyas = s.subidas.entry(jugador).or_default();
        if !suyas.contains(&ruta) && suyas.len() >= MAX_SUBIDAS_POR_JUGADOR {
            return Err("Has subido demasiadas imágenes".into());
        }
        suyas.insert(ruta);
        Ok(())
    }

    /// Ping y recorrido del host: a todos, si son de un mapa compartido.
    pub fn difundir(&self, aviso: &AvisoTablero, compartidos: &[u16]) {
        if !aviso.id_mapa().is_some_and(|m| compartidos.contains(&m)) {
            return; // el foco del host no se comparte, ni lo de mapas que no ven
        }
        if let Some(s) = self.dentro().as_ref() {
            for c in s.conexiones.values() {
                let _ = c.enviar.send(MensajeHost::Aviso { aviso: aviso.clone() });
            }
        }
    }

    // ------------------------------------------------------------------
    // Para el servidor
    // ------------------------------------------------------------------

    fn nuevo_id(&self) -> u64 {
        self.siguiente_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Anota un fallo de contraseña. Devuelve si aún se puede intentar (no se pasó del límite por minuto).
    fn anotar_fallo(&self) -> bool {
        let mut dentro = self.dentro();
        let Some(s) = dentro.as_mut() else { return false };
        let ahora = Instant::now();
        while s.fallos.front().is_some_and(|t| ahora.duration_since(*t) > Duration::from_secs(60)) {
            s.fallos.pop_front();
        }
        s.fallos.push_back(ahora);
        s.fallos.len() <= FALLOS_POR_MINUTO
    }

    fn demasiados_fallos(&self) -> bool {
        let dentro = self.dentro();
        let Some(s) = dentro.as_ref() else { return true };
        let ahora = Instant::now();
        s.fallos.iter().filter(|t| ahora.duration_since(**t) <= Duration::from_secs(60)).count() >= FALLOS_POR_MINUTO
    }

    /// Da de alta la conexión de `id` con su vista inicial. Se llama con el estado cogido (`est`), así ningún
    /// delta se cuela entre la vista enviada y la guardada. Una conexión anterior del mismo jugador se cierra.
    fn conectar(&self, est: &EstadoPartida, id: IdJugador, enviar: mpsc::UnboundedSender<MensajeHost>) -> Option<u64> {
        let vista = vista_para(id, est);
        let mut dentro = self.dentro();
        let s = dentro.as_mut()?;
        let id_conexion = self.nuevo_id();
        let _ = enviar.send(MensajeHost::Vista { vista: vista.clone() });
        let anterior = s.conexiones.insert(id, Conexion { id_conexion, enviar, ultima: vista });
        if let Some(a) = anterior {
            let _ = a.enviar.send(MensajeHost::Rechazado { motivo: "Has entrado desde otra pestaña o dispositivo".into() });
        }
        Some(id_conexion)
    }

    fn desconectar(&self, id: IdJugador, id_conexion: u64) {
        let quitada = {
            let mut dentro = self.dentro();
            let Some(s) = dentro.as_mut() else { return };
            match s.conexiones.get(&id) {
                Some(c) if c.id_conexion == id_conexion => s.conexiones.remove(&id).is_some(),
                _ => false,
            }
        };
        if quitada {
            self.avisar();
        }
    }

    /// La sesión de imágenes de `id`: la que ya tenía (si se reconecta) o una nueva.
    fn sesion_de(&self, id: IdJugador) -> Option<String> {
        let mut dentro = self.dentro();
        let s = dentro.as_mut()?;
        Some(s.sesiones.entry(id).or_insert_with(|| SalaGuardada::nueva().secreto).clone())
    }

    /// El jugador de una sesión (para servir imágenes).
    fn jugador_de_sesion(&self, sesion: &str) -> Option<IdJugador> {
        let dentro = self.dentro();
        let s = dentro.as_ref()?;
        // Se recorren todas en tiempo constante (que el tiempo no diga cuánto se acertó)
        let mut encontrado = None;
        for (id, x) in &s.sesiones {
            if sala::iguales(x, sesion) {
                encontrado = Some(*id);
            }
        }
        encontrado
    }

    /// Si se acepta una página de este origen (el WebSocket y el CORS de `/imagen`): la de GitHub Pages (y, en
    /// desarrollo, localhost) o la que sirve la propia sala abierta en su dirección.
    pub(crate) fn acepta_origen(&self, origen: &str) -> bool {
        self.origenes.iter().any(|o| o == origen)
            || self.dentro().as_ref().is_some_and(|s| s.origenes_propios().iter().any(|o| o.eq_ignore_ascii_case(origen)))
    }

    /// Carpeta de la partida de la sala abierta.
    fn dir_sala(&self) -> Option<PathBuf> {
        self.dentro().as_ref().map(|s| s.dir.clone())
    }
}

#[cfg(test)]
impl Red {
    /// La sala abierta sin túnel (solo el servidor local), para los tests del servidor.
    pub(crate) async fn abrir_local(self: &Arc<Self>) -> u16 {
        let dir = self.almacen.dir();
        let mut sala = SalaGuardada::leer_o_crear(&dir).unwrap();
        sala.asegurar_certificado().unwrap();
        *self.dentro() = Some(SalaAbierta {
            sala,
            dir,
            fase: Fase::Abierta,
            tunel: Some("prueba.local".into()),
            conexiones: HashMap::new(),
            sesiones: HashMap::new(),
            subidas: HashMap::new(),
            fallos: VecDeque::new(),
            apagar_servidor: None,
            proceso_tunel: None,
            generacion: 0,
            respaldo: config::Respaldo::Cloudflare,
            lan: Vec::new(),
            codigos_vistos: VecDeque::new(),
        });
        let (puerto, apagar) = servidor::arrancar(self.clone(), std::net::Ipv4Addr::LOCALHOST, 0).await.unwrap();
        self.dentro().as_mut().unwrap().apagar_servidor = Some(apagar);
        puerto
    }

    /// La sala marcada como abierta, sin servidor ni túnel (para probar lo que bloquea).
    pub(crate) fn abrir_sin_servidor(&self) {
        let dir = self.almacen.dir();
        *self.dentro() = Some(SalaAbierta {
            sala: SalaGuardada::nueva(),
            dir,
            fase: Fase::Abierta,
            tunel: None,
            conexiones: HashMap::new(),
            sesiones: HashMap::new(),
            subidas: HashMap::new(),
            fallos: VecDeque::new(),
            apagar_servidor: None,
            proceso_tunel: None,
            generacion: 0,
            respaldo: config::Respaldo::Cloudflare,
            lan: Vec::new(),
            codigos_vistos: VecDeque::new(),
        });
    }

    /// Cambia cómo está publicada la sala abierta (para probar lo que depende de ello).
    pub(crate) fn poner_respaldo_para_tests(&self, r: config::Respaldo) {
        self.dentro().as_mut().unwrap().respaldo = r;
    }

    pub(crate) fn sala_para_tests(&self) -> SalaGuardada {
        self.dentro().as_ref().map(|s| s.sala.clone()).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tablero_general::almacen::pruebas::dir_temporal;

    fn red_en(nombre: &str) -> Arc<Red> {
        let dir = dir_temporal(nombre);
        let almacen = Arc::new(Almacen::abrir(&dir).unwrap());
        Arc::new(Red::nueva(Arc::new(Mutex::new(EstadoPartida::default())), almacen, dir))
    }

    fn puerto_libre() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
    }

    #[tokio::test]
    async fn con_direccion_propia_se_abre_sin_tunel_en_su_puerto_y_el_enlace_la_lleva() {
        let red = red_en("red_propia");
        let puerto = puerto_libre();
        let propia = config::ConfigRed { respaldo: config::Respaldo::Propio { url: "https://Mesa.Mi-Dominio.es/".into(), puerto } };
        red.guardar_config(&propia).unwrap();
        assert_eq!(red.estado(&EstadoPartida::default()).respaldo, propia.respaldo, "cerrada: enseña la que usará");

        red.abrir().await.unwrap();
        let e = red.estado(&EstadoPartida::default());
        assert_eq!(e.fase, Fase::Abierta);
        // El enlace es la propia dirección (la página la sirve la app): sin GitHub Pages ni `t`
        let enlace = e.enlace.clone().unwrap();
        assert!(enlace.starts_with("https://mesa.mi-dominio.es/#s="), "{enlace}");
        assert!(!enlace.contains("t="), "{enlace}");
        // Peer to peer: lleva la huella del certificado de la conexión directa, que se guarda en sala.json
        let huella = enlace.split("&f=").nth(1).expect("lleva la huella").to_string();
        assert_eq!(huella.len(), 43);
        assert_eq!(SalaGuardada::leer_o_crear(&red.almacen.dir()).unwrap().huella().as_deref(), Some(huella.as_str()));
        red.regenerar_enlace().unwrap();
        let nuevo = red.estado(&EstadoPartida::default()).enlace.unwrap();
        assert!(!nuevo.contains(&huella), "un enlace nuevo trae otra identidad: {nuevo}");
        assert_eq!(e.enlaces.len(), 1);
        assert!(red.acepta_origen("https://mesa.mi-dominio.es"), "su página");
        assert!(!red.acepta_origen("https://otro.es"));
        // Escucha en ese puerto, en 127.0.0.1 (lo que el host lleve hasta ahí es cosa suya)
        assert!(tokio::net::TcpStream::connect(("127.0.0.1", puerto)).await.is_ok());
        assert!(red.guardar_config(&config::ConfigRed::default()).is_err(), "abierta no se cambia");
        red.cerrar();
    }

    #[tokio::test]
    async fn en_red_local_escucha_en_todas_las_interfaces_y_da_un_enlace_por_ip() {
        let red = red_en("red_lan");
        let puerto = puerto_libre();
        red.guardar_config(&config::ConfigRed { respaldo: config::Respaldo::Lan { puerto } }).unwrap();
        red.abrir().await.unwrap();
        let e = red.estado(&EstadoPartida::default());
        assert_eq!(e.fase, Fase::Abierta);
        let lan = config::direcciones_lan();
        assert_eq!(e.enlaces.len(), lan.len(), "un enlace por cada IP del equipo");
        for (d, enlace) in lan.iter().zip(&e.enlaces) {
            assert_eq!(enlace.etiqueta, d.interfaz);
            assert_eq!(enlace.url, format!("http://{}:{puerto}/", d.ip), "sin llave ni huella: solo ip y puerto");
        }
        assert!(SalaGuardada::leer_o_crear(&red.almacen.dir()).unwrap().certificado_dtls.is_none(), "en red local no hay conexión directa");
        // Escucha también fuera de 127.0.0.1 (en cualquier IP del equipo)
        if let Some(d) = lan.first() {
            assert!(tokio::net::TcpStream::connect((d.ip, puerto)).await.is_ok());
            assert!(red.acepta_origen(&format!("http://{}:{puerto}", d.ip)));
        }
        assert!(red.acepta_origen(&format!("http://localhost:{puerto}")));
        assert!(!red.acepta_origen(&format!("http://otro.es:{puerto}")), "un origen que no es suyo");
        assert!(!red.acepta_origen(&format!("https://localhost:{puerto}")), "en red local es http");
        red.cerrar();
    }

    #[tokio::test]
    async fn la_prueba_del_token_vale_para_sus_direcciones_y_no_para_otra() {
        let red = red_en("red_prueba");
        let puerto = puerto_libre();
        red.guardar_config(&config::ConfigRed { respaldo: config::Respaldo::Lan { puerto } }).unwrap();
        red.abrir().await.unwrap();
        let token = red.cambiar_sala(|s| Ok(s.nuevo_token(7))).unwrap();
        let dentro = red.dentro();
        let s = dentro.as_ref().unwrap();
        let vale = |direccion: &str| {
            let p = SalaGuardada::responder(&token, "reto", direccion);
            s.direcciones().iter().find_map(|d| s.sala.jugador_de_prueba("reto", d, &p))
        };
        assert_eq!(vale(&format!("localhost:{puerto}")), Some(7));
        assert_eq!(vale(&format!("127.0.0.1:{puerto}")), Some(7));
        assert_eq!(vale("servidor-falso.es"), None, "una prueba hecha para otro servidor no vale");
        drop(dentro);
        red.cerrar();
    }

    #[tokio::test]
    async fn con_el_puerto_ocupado_no_se_abre_y_se_dice_por_que() {
        let red = red_en("red_ocupado");
        let ocupado = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let puerto = ocupado.local_addr().unwrap().port();
        red.guardar_config(&config::ConfigRed { respaldo: config::Respaldo::Propio { url: "https://mi.dominio.es".into(), puerto } }).unwrap();
        let error = red.abrir().await.unwrap_err();
        assert!(error.contains("ocupado"), "{error}");
        let e = red.estado(&EstadoPartida::default());
        assert_eq!(e.fase, Fase::Cerrada);
        assert_eq!(e.error.as_deref(), Some(error.as_str()));
    }

    #[test]
    fn el_enlace_lleva_todo_tras_el_almohadilla_y_el_origen_se_saca_bien() {
        let sala = SalaGuardada { id: "S".into(), secreto: "K".into(), hash_contrasena: None, tokens: vec![], certificado_dtls: None };
        assert_eq!(enlace("https://a.github.io/vtt-rol/", "x.trycloudflare.com", &sala), "https://a.github.io/vtt-rol/#t=x.trycloudflare.com&s=S&k=K");
        assert_eq!(origen("https://a.github.io/vtt-rol/"), "https://a.github.io");
        assert_eq!(origen("http://localhost:5173/"), "http://localhost:5173");
    }
}
