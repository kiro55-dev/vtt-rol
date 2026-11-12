// src-tauri/src/red/imagenes.rs
//
// Las imágenes que piden y suben los jugadores, con UNA sola regla para las dos formas de pedirlas:
//  * por el protocolo (`PedirImagen` / `SubirImagen`, en trozos por la misma conexión del juego), lo normal;
//  * por HTTP (`/imagen`, `/subir` de `servidor.rs`), el respaldo para navegadores sin Service Worker.
//
// Un jugador solo recibe lo que ve (`visibilidad::recurso_visible_para`) o lo que ha subido él en esta sala, y solo
// sube imágenes de verdad con tope (`imagenes::importar_bytes`, `Red::anotar_subida`).
use std::path::PathBuf;

use crate::red::Red;
use crate::tablero_general::modelos::IdJugador;
use crate::tablero_general::{archivos, miniaturas, visibilidad};

/// Tamaño de cada trozo de imagen por el protocolo. Es el que va bien en cualquier navegador por WebRTC (fase 3);
/// por el WebSocket da igual, pero así hay un solo camino. Mismo valor en `src/lib/red/imagenesRed.ts`.
pub const TROZO: usize = 16 * 1024;

/// Tipo MIME por la extensión (las imágenes se guardan con la suya, comprobada al importarlas).
pub fn tipo_mime(ruta: &str) -> &'static str {
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

/// El archivo que `jugador` puede recibir para `ruta` (con `mini` = "ficha"/"mapa", su miniatura, que se genera si
/// hace falta) y su tipo MIME. `None` si no la ve o no existe: las dos cosas se contestan igual, para no dar pistas.
/// Bloqueante (puede generar la miniatura): llamar desde `spawn_blocking`.
pub fn localizar(red: &Red, jugador: IdJugador, ruta: &str, mini: Option<&str>) -> Option<(PathBuf, &'static str)> {
    let dir = red.dir_sala()?;
    let visible = visibilidad::recurso_visible_para(jugador, &red.estado_partida(), ruta)
        || red.subidas_de(jugador).contains(ruta); // lo que acaba de subir, aunque aún no lo use ninguna ficha
    if !visible {
        return None;
    }
    let final_ = match mini.and_then(miniaturas::Tipo::desde_texto) {
        Some(t) => miniaturas::asegurar(&dir, ruta, t).ok()?.unwrap_or_else(|| ruta.to_string()),
        None => ruta.to_string(),
    };
    let camino = archivos::ruta_segura(&dir, &final_, &["imagenes", "miniaturas"]).ok()?;
    camino.is_file().then(|| (camino, tipo_mime(&final_)))
}

/// Guarda una imagen subida por `jugador` y la anota como suya. Devuelve su ruta ("imagenes/…"). Bloqueante.
pub fn guardar_subida(red: &Red, jugador: IdJugador, bytes: &[u8], ext: &str) -> Result<String, String> {
    let dir = red.dir_sala().ok_or("La sala se ha cerrado")?;
    let ruta = crate::tablero_general::imagenes::importar_bytes(&dir, bytes, ext)?;
    red.anotar_subida(jugador, ruta.clone())?;
    Ok(ruta)
}

// ------------------------------------------------------------------
// Por el protocolo
// ------------------------------------------------------------------

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::AsyncReadExt;
use tokio::sync::{mpsc, Semaphore};

use crate::red::protocolo::{MensajeHost, TROZO_BAJADA, TROZO_SUBIDA};
use crate::red::transporte::{self, Trama};
use crate::tablero_general::almacen::bloquear;
use crate::tablero_general::imagenes::MAX_BYTES_SUBIDA;

/// Imágenes que se mandan a la vez a un jugador, y cuántas pueden esperar turno. Más → `ImagenError("ocupado")`.
pub const MAX_BAJADAS_A_LA_VEZ: usize = 4;
pub const MAX_BAJADAS_EN_COLA: usize = 64;
/// Subidas de un jugador a medias a la vez.
pub const MAX_SUBIDAS_A_LA_VEZ: usize = 2;
/// Una subida sin trozos nuevos en este tiempo se descarta.
pub const TIEMPO_SUBIDA: Duration = Duration::from_secs(60);
/// Lo que contesta el host si ya tiene demasiadas imágenes en marcha (el navegador reintenta).
pub const OCUPADO: &str = "ocupado";

/// Un trozo binario: tipo + id (big endian) + datos.
pub fn trozo(tipo: u8, id: u32, datos: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(5 + datos.len());
    v.push(tipo);
    v.extend_from_slice(&id.to_be_bytes());
    v.extend_from_slice(datos);
    v
}

/// Lee la cabecera de un trozo binario: (tipo, id, datos). `None` si es demasiado corto.
pub fn leer_trozo(b: &[u8]) -> Option<(u8, u32, &[u8])> {
    let id = u32::from_be_bytes(b.get(1..5)?.try_into().ok()?);
    Some((b[0], id, &b[5..]))
}

/// Las imágenes que se están mandando a UN jugador (una por conexión). Al soltarlo (fin de la conexión) se
/// cancelan todas: si no, sus tareas mantendrían viva la salida del transporte.
pub struct Bajadas {
    turno: Arc<Semaphore>,
    /// Pedidas que aún no han terminado (en marcha + esperando turno).
    pendientes: Arc<AtomicUsize>,
    control: Control,
}

/// Lo que mira cada envío para saber si debe parar.
#[derive(Clone, Default)]
struct Control {
    canceladas: Arc<Mutex<HashSet<u32>>>,
    cerrada: Arc<AtomicBool>,
}

impl Control {
    fn parar(&self, id: u32) -> bool {
        self.cerrada.load(Ordering::Relaxed) || bloquear(&self.canceladas).contains(&id)
    }
}

impl Default for Bajadas {
    fn default() -> Self {
        Bajadas {
            turno: Arc::new(Semaphore::new(MAX_BAJADAS_A_LA_VEZ)),
            pendientes: Arc::new(AtomicUsize::new(0)),
            control: Control::default(),
        }
    }
}

impl Drop for Bajadas {
    fn drop(&mut self) {
        self.control.cerrada.store(true, Ordering::Relaxed);
        self.turno.close(); // las que esperaban turno ya no empiezan
    }
}

impl Bajadas {
    /// Atiende un `PedirImagen`: en su turno, comprueba que la ve, manda la cabecera y luego los trozos, al ritmo
    /// que permita la conexión (la salida está acotada). No bloquea a quien llama.
    pub fn pedir(&self, red: Arc<Red>, jugador: IdJugador, id: u32, ruta: String, mini: Option<String>, tx: mpsc::Sender<Trama>) {
        if self.pendientes.fetch_add(1, Ordering::Relaxed) >= MAX_BAJADAS_A_LA_VEZ + MAX_BAJADAS_EN_COLA {
            self.pendientes.fetch_sub(1, Ordering::Relaxed);
            tokio::spawn(async move {
                let _ = transporte::enviar_mensaje(&tx, &MensajeHost::ImagenError { id, error: OCUPADO.into() }).await;
            });
            return;
        }
        let (turno, pendientes, control) = (self.turno.clone(), self.pendientes.clone(), self.control.clone());
        tokio::spawn(async move {
            if let Ok(_permiso) = turno.acquire_owned().await {
                mandar(&red, jugador, id, &ruta, mini.as_deref(), &tx, &control).await;
            }
            bloquear(&control.canceladas).remove(&id);
            pendientes.fetch_sub(1, Ordering::Relaxed);
        });
    }

    pub fn cancelar(&self, id: u32) {
        bloquear(&self.control.canceladas).insert(id);
    }
}

async fn mandar(
    red: &Arc<Red>,
    jugador: IdJugador,
    id: u32,
    ruta: &str,
    mini: Option<&str>,
    tx: &mpsc::Sender<Trama>,
    control: &Control,
) {
    let cancelada = || control.parar(id);
    if cancelada() {
        return;
    }
    let (r, ruta_b, mini_b) = (red.clone(), ruta.to_string(), mini.map(String::from));
    let encontrado = tokio::task::spawn_blocking(move || localizar(&r, jugador, &ruta_b, mini_b.as_deref())).await.ok().flatten();
    let abierto = match encontrado {
        Some((camino, mime)) => tokio::fs::File::open(&camino).await.ok().map(|f| (f, mime)),
        None => None,
    };
    let Some((mut archivo, mime)) = abierto else {
        let _ = transporte::enviar_mensaje(tx, &MensajeHost::ImagenError { id, error: "No disponible".into() }).await;
        return;
    };
    let tam = archivo.metadata().await.map(|m| m.len()).unwrap_or(0);
    if transporte::enviar_mensaje(tx, &MensajeHost::ImagenCabecera { id, mime: mime.into(), tam }).await.is_err() {
        return;
    }
    // Trozo a trozo desde el disco: ni la imagen entera en memoria ni más de lo que la conexión acepta
    let mut buf = vec![0u8; TROZO];
    let mut enviados = 0u64;
    while enviados < tam {
        if cancelada() {
            return;
        }
        let mut lleno = 0;
        while lleno < TROZO {
            match archivo.read(&mut buf[lleno..]).await {
                Ok(0) => break,
                Ok(n) => lleno += n,
                Err(_) => return,
            }
        }
        if lleno == 0 {
            return; // el archivo menguó: el navegador lo dará por fallido al no completarse
        }
        if transporte::enviar_trama(tx, Trama::Binario(trozo(TROZO_BAJADA, id, &buf[..lleno]))).await.is_err() {
            return;
        }
        enviados += lleno as u64;
    }
}

/// Una imagen que está subiendo un jugador.
struct Subida {
    ext: String,
    tam: usize,
    datos: Vec<u8>,
    ultimo: Instant,
}

/// Las subidas de UN jugador (una por conexión).
#[derive(Default)]
pub struct Subidas {
    en_curso: HashMap<u32, Subida>,
}

/// Lo que pasa con una subida al llegarle un trozo.
#[derive(Debug, PartialEq)]
pub enum FinSubida {
    /// Completa: (id, datos, extensión) para guardarla.
    Completa(u32, Vec<u8>, String),
    /// Se pasó de lo anunciado: se descarta.
    Mal(u32, String),
}

impl Subidas {
    /// `SubirImagen`: empieza una subida. `Err` con el motivo si no se admite.
    pub fn empezar(&mut self, id: u32, ext: String, tam: usize) -> Result<(), String> {
        if tam == 0 || tam > MAX_BYTES_SUBIDA {
            return Err(format!("La imagen no puede pasar de {} MB", MAX_BYTES_SUBIDA / (1024 * 1024)));
        }
        if self.en_curso.len() >= MAX_SUBIDAS_A_LA_VEZ || self.en_curso.contains_key(&id) {
            return Err("Ya estás subiendo otra imagen".into());
        }
        // Sin reservar `tam` de antemano: la memoria crece con lo que llega de verdad
        self.en_curso.insert(id, Subida { ext, tam, datos: Vec::new(), ultimo: Instant::now() });
        Ok(())
    }

    /// Un trozo binario de subida. `Some` si con él la subida termina (bien o mal); `None` si sigue a medias o el
    /// trozo no es de ninguna.
    pub fn trozo(&mut self, b: &[u8]) -> Option<FinSubida> {
        let (tipo, id, datos) = leer_trozo(b)?;
        if tipo != TROZO_SUBIDA {
            return None;
        }
        let s = self.en_curso.get_mut(&id)?;
        if s.datos.len() + datos.len() > s.tam {
            self.en_curso.remove(&id);
            return Some(FinSubida::Mal(id, "La imagen no tiene el tamaño anunciado".into()));
        }
        s.datos.extend_from_slice(datos);
        s.ultimo = Instant::now();
        if s.datos.len() < s.tam {
            return None;
        }
        let s = self.en_curso.remove(&id)?;
        Some(FinSubida::Completa(id, s.datos, s.ext))
    }

    /// Descarta las subidas sin trozos nuevos en `TIEMPO_SUBIDA`. Devuelve sus ids.
    pub fn caducar(&mut self) -> Vec<u32> {
        let viejas: Vec<u32> = self.en_curso.iter().filter(|(_, s)| s.ultimo.elapsed() > TIEMPO_SUBIDA).map(|(id, _)| *id).collect();
        for id in &viejas {
            self.en_curso.remove(id);
        }
        viejas
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_trozos_llevan_tipo_e_id() {
        let t = trozo(TROZO_BAJADA, 0x01020304, b"abc");
        assert_eq!(t, vec![1, 1, 2, 3, 4, b'a', b'b', b'c']);
        assert_eq!(leer_trozo(&t), Some((TROZO_BAJADA, 0x01020304, &b"abc"[..])));
        assert_eq!(leer_trozo(&[1, 2]), None, "demasiado corto");
    }

    #[test]
    fn una_subida_termina_al_llegar_su_tamano_y_no_admite_mas() {
        let mut s = Subidas::default();
        assert!(s.empezar(1, "png".into(), 0).is_err());
        assert!(s.empezar(1, "png".into(), MAX_BYTES_SUBIDA + 1).is_err());
        s.empezar(1, "png".into(), 5).unwrap();
        assert!(s.empezar(1, "png".into(), 5).is_err(), "mismo id");
        s.empezar(2, "png".into(), 3).unwrap();
        assert!(s.empezar(3, "png".into(), 3).is_err(), "tope de subidas a la vez");

        assert_eq!(s.trozo(&trozo(TROZO_SUBIDA, 1, b"ab")), None, "a medias");
        assert_eq!(s.trozo(&trozo(TROZO_SUBIDA, 9, b"ab")), None, "de ninguna");
        assert_eq!(s.trozo(&trozo(TROZO_BAJADA, 1, b"ab")), None, "no es de subida");
        assert_eq!(s.trozo(&trozo(TROZO_SUBIDA, 1, b"cde")), Some(FinSubida::Completa(1, b"abcde".to_vec(), "png".into())));
        assert!(matches!(s.trozo(&trozo(TROZO_SUBIDA, 2, b"wxyz")), Some(FinSubida::Mal(2, _))), "más de lo anunciado");
        assert!(s.en_curso.is_empty());
        assert!(s.caducar().is_empty());
    }
}
