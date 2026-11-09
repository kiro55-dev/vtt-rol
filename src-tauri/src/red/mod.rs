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
pub mod peticiones;
pub mod protocolo;
pub mod sala;
pub mod servidor;
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
}

impl SalaAbierta {
    fn enlace(&self) -> Option<String> {
        self.tunel.as_ref().map(|t| enlace(URL_CLIENTE, t, &self.sala))
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
}

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
                enlace: s.enlace(),
                pide_contrasena: s.sala.pide_contrasena(),
                conectados: s.conexiones.keys().copied().collect(),
                mapas_compartidos,
                error,
            },
            None => EstadoSala {
                fase: Fase::Cerrada,
                enlace: None,
                pide_contrasena: SalaGuardada::leer_o_crear(&self.almacen.dir()).is_ok_and(|s| s.pide_contrasena()),
                conectados: Vec::new(),
                mapas_compartidos,
                error,
            },
        }
    }

    // ------------------------------------------------------------------
    // Abrir y cerrar
    // ------------------------------------------------------------------

    /// Abre la sala: servidor local, cloudflared (lo descarga la primera vez) y túnel. Avisa al front en
    /// cada paso. Si se cierra a mitad, lo que se estaba preparando se descarta.
    pub async fn abrir(self: &Arc<Self>) -> Result<(), String> {
        let generacion = self.siguiente_id.fetch_add(1, Ordering::Relaxed);
        {
            let mut dentro = self.dentro();
            if dentro.is_some() {
                return Ok(());
            }
            let dir = self.almacen.dir();
            let sala = SalaGuardada::leer_o_crear(&dir)?;
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
            });
        }
        *bloquear(&self.ultimo_error) = None;
        self.avisar();

        let resultado = self.preparar(generacion).await;
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
    async fn preparar(self: &Arc<Self>, generacion: u64) -> Result<(), String> {
        let (puerto, apagar) = servidor::arrancar(self.clone()).await?;
        if !self.con_sala(generacion, |s| s.apagar_servidor = Some(apagar)) {
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
        self.avisar();
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
        if let Some(s) = self.dentro().as_mut() {
            s.sesiones.clear();
            s.subidas.clear();
            for (_, c) in s.conexiones.drain() {
                let _ = c.enviar.send(MensajeHost::Rechazado { motivo: "El host ha cambiado el enlace de la sala".into() });
            }
        }
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
        if let Some(c) = self.dentro().as_mut().and_then(|s| {
            s.sesiones.remove(&id);
            s.subidas.remove(&id);
            s.conexiones.remove(&id)
        }) {
            let _ = c.enviar.send(MensajeHost::Rechazado { motivo: "El host te ha sacado de la sala".into() });
        }
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
        s.sesiones.iter().find(|(_, x)| x.as_str() == sesion).map(|(id, _)| *id)
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
        let sala = SalaGuardada::leer_o_crear(&dir).unwrap();
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
        });
        let (puerto, apagar) = servidor::arrancar(self.clone()).await.unwrap();
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
        });
    }

    pub(crate) fn sala_para_tests(&self) -> SalaGuardada {
        self.dentro().as_ref().map(|s| s.sala.clone()).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_enlace_lleva_todo_tras_el_almohadilla_y_el_origen_se_saca_bien() {
        let sala = SalaGuardada { id: "S".into(), secreto: "K".into(), hash_contrasena: None, tokens: vec![] };
        assert_eq!(enlace("https://a.github.io/vtt-rol/", "x.trycloudflare.com", &sala), "https://a.github.io/vtt-rol/#t=x.trycloudflare.com&s=S&k=K");
        assert_eq!(origen("https://a.github.io/vtt-rol/"), "https://a.github.io");
        assert_eq!(origen("http://localhost:5173/"), "http://localhost:5173");
    }
}
