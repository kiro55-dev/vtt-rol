// src-tauri/src/tablero_general/partida.rs
//
// Comandos de Tauri y estado compartido. La lógica de estado/deltas está en `estado.rs`
// y el acceso a disco en `almacen.rs`.
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::tablero_general::almacen::{bloquear, Almacen};
use crate::tablero_general::estado::{self, DeltaPartida, EstadoPartida};
use crate::tablero_general::imagenes;
use crate::tablero_general::modelos::*;
use crate::tablero_general::archivos;
use std::collections::HashSet;

pub struct AppState {
    pub estado: Mutex<EstadoPartida>,
    pub almacen: Almacen,
}

impl AppState {
    pub fn lock(&self) -> MutexGuard<'_, EstadoPartida> {
        bloquear(&self.estado)
    }
}

/// Tarea en segundo plano: espera 2 segundos tras arrancar la app y purga
/// silenciosamente cualquier archivo huérfano en un hilo de fondo sin tocar la UI.
pub fn iniciar_limpieza_huerfanos(app_handle: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // Pausa de cortesía para que la interfaz cargue y renderice primero
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        if let Some(state) = app_handle.try_state::<AppState>() {
            let dir = state.almacen.dir().to_path_buf();
            
            // Obtenemos en microsegundos el conjunto de rutas válidas
            let archivos_validos: HashSet<String> = {
                let est = state.lock();
                est.banco_entidades
                    .iter()
                    .flat_map(|e| e.archivos_adjuntos.iter().flatten().cloned())
                    .collect()
            };

            // Ejecuta el barrido de disco en un hilo de bloqueo fuera del hilo principal
            let _ = tauri::async_runtime::spawn_blocking(move || {
                archivos::purgar_archivos_no_referenciados(&dir, &archivos_validos);
            }).await;
        }
    });
}

// ========================================================
// DESPACHO DE CAMBIOS
// ========================================================

/// Registra un delta con el lock YA cogido (para comandos que leen y escriben a la vez).
fn registrar(est: &mut EstadoPartida, almacen: &Almacen, delta: DeltaPartida) {
    // FUTURA RED: aquí es donde enviarás `delta` por websocket/red.
    if let Err(e) = est.registrar(almacen, delta) {
        eprintln!("[partida] {e}");
    }
}

/// Punto de entrada para cualquier cambio: lo aplica en RAM y lo añade al log, en ese orden
/// y bajo el mismo lock, así el orden del log siempre coincide con el del estado.
pub fn procesar_delta(state: &AppState, delta: DeltaPartida) {
    let mut est = state.lock();
    registrar(&mut est, &state.almacen, delta);
}

// Función específica para registrar tiradas (llamada por dados.rs)
pub fn registrar_tirada(state: &AppState, resultado: &str) {
    let (desglose, total_num) = if let Some((izq, der)) = resultado.split_once(" = ") {
        (izq.to_string(), der.trim().parse::<i32>().unwrap_or(0))
    } else {
        (resultado.to_string(), 0)
    };

    let tirada = Tiradas {
        total: total_num,
        comando: desglose,
        hora: chrono::Local::now().format("%H:%M").to_string(),
    };

    procesar_delta(state, DeltaPartida::TiradaRealizada(tirada));
}

// Historial personal de comandos (dados.rs)
pub fn registrar_comando_personal(state: &AppState, comando: &str) {
    state.almacen.anadir_historial(comando);
}

// ========================================================
// GUARDADO
// ========================================================

/// Vuelca la RAM a partida.json (atómico) y vacía el log. Mantiene el lock durante la
/// operación a propósito: si se soltara antes de vaciar el log, un delta llegado entre
/// medias se perdería.
pub fn compactar_partida(state: &AppState) {
    let est = state.lock();
    if let Err(e) = estado::compactar(&state.almacen, &est) {
        eprintln!("[partida] {e}");
    }
}

// BUCLE DE AUTOGUARDADO CADA 10 MINUTOS
pub fn iniciar_autoguardado(app_handle: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut intervalo = tokio::time::interval(std::time::Duration::from_secs(600));
        intervalo.tick().await; // el primer tick es inmediato: lo descartamos

        loop {
            intervalo.tick().await;
            if let Some(state) = app_handle.try_state::<AppState>() {
                compactar_partida(&state);
            }
        }
    });
}

// Comando opcional por si se implementa botón de guardar partida
#[tauri::command]
pub async fn _guardar_partida(state: State<'_, AppState>) -> Result<(), String> {
    compactar_partida(&state);
    Ok(())
}

// ========================================================
// COMANDOS DE LECTURA (leen de RAM)
// ========================================================

#[tauri::command]
pub fn obtener_historial_tiradas(state: State<'_, AppState>) -> Vec<Tiradas> {
    state.lock().historial_tiradas.clone()
}

#[tauri::command]
pub fn obtener_zonas(state: State<'_, AppState>) -> Vec<Zona> {
    state.lock().zonas.clone()
}

#[tauri::command]
pub fn obtener_mapas_zona(zona_id: u16, state: State<'_, AppState>) -> Vec<Mapa> {
    state.lock().mapas.iter().filter(|m| m.id_zona == zona_id).cloned().collect()
}

#[tauri::command]
pub fn obtener_banco_entidades(state: State<'_, AppState>) -> Vec<Entidad> {
    state.lock().banco_entidades.clone()
}

#[tauri::command]
pub fn obtener_instancias_activas(state: State<'_, AppState>) -> Vec<Instancia> {
    state.lock().instancias_activas.clone()
}

#[tauri::command]
pub fn obtener_historial_personal(state: State<'_, AppState>) -> Vec<String> {
    state.almacen.leer_historial()
}

// ========================================================
// ZONAS Y MAPAS
// ========================================================

#[tauri::command]
pub fn crear_zona(state: State<'_, AppState>) -> Zona {
    // Calcular el id y registrar el delta en la MISMA sección crítica: dos llamadas
    // simultáneas ya no pueden obtener el mismo id.
    let mut est = state.lock();

    let id = est.zonas.iter().map(|z| z.id).max().unwrap_or(0) + 1;
    let orden = est.zonas.iter().map(|z| z.orden).max().map(|o| o + 1).unwrap_or(0);

    let nueva_zona = Zona {
        id,
        nombre: format!("Zona {}", id),
        orden,
    };

    registrar(&mut est, &state.almacen, DeltaPartida::ZonaCreada(nueva_zona.clone()));
    nueva_zona
}

#[tauri::command]
pub fn eliminar_zona(zona: Zona, state: State<'_, AppState>) {
    procesar_delta(&state, DeltaPartida::ZonaEliminada(zona));
}

#[tauri::command]
pub fn modificar_zona(zona: Zona, state: State<'_, AppState>) {
    procesar_delta(&state, DeltaPartida::ZonaModificada(zona));
}

/// `url_imagen_fondo` es la ruta RELATIVA devuelta por `importar_imagen`
/// (p. ej. "imagenes/ab12...png"), no una URL ni una ruta del sistema.
#[tauri::command]
pub fn crear_mapa(
    id_zona: u16,
    url_imagen_fondo: Option<String>,
    state: State<'_, AppState>,
) -> Result<Mapa, String> {
    let mut est = state.lock();

    let zona = est
        .zonas
        .iter()
        .find(|z| z.id == id_zona)
        .cloned()
        .ok_or_else(|| format!("La zona {} no existe", id_zona))?;

    // ID secuencial global para evitar cualquier colisión entre zonas
    let id = est.mapas.iter().map(|m| m.id).max().unwrap_or(0) + 1;
    let orden = est
        .mapas
        .iter()
        .filter(|m| m.id_zona == id_zona)
        .map(|m| m.orden)
        .max()
        .map(|o| o + 1)
        .unwrap_or(0);

    let nuevo_mapa = Mapa {
        id,
        id_zona,
        nombre: id.to_string(), // Nombre por defecto es su propio ID
        url_imagen_fondo,
        ancho: 1000,
        alto: 1000,
        rejilla: 50,
        color_opacidad: vec![255, 255, 255, 76],
        grosor: 1,
        orden,
    };

    registrar(&mut est, &state.almacen, DeltaPartida::MapaCreado(zona, nuevo_mapa.clone()));
    Ok(nuevo_mapa)
}

#[tauri::command]
pub fn modificar_mapa(mapa: Mapa, state: State<'_, AppState>) {
    procesar_delta(&state, DeltaPartida::MapaModificado(mapa));
}

#[tauri::command]
pub fn borrar_mapa(mapa: Mapa, state: State<'_, AppState>) {
    procesar_delta(&state, DeltaPartida::MapaBorrado(mapa));
}

// ========================================================
// IMÁGENES
// ========================================================

/// Directorio de datos de la app. El front lo usa para construir la URL de las imágenes
/// guardadas como ruta relativa (ver src/lib/imagenes.ts).
#[tauri::command]
pub fn obtener_dir_datos(state: State<'_, AppState>) -> String {
    state.almacen.dir().to_string_lossy().into_owned()
}

/// Copia la imagen elegida por el usuario al directorio de datos y devuelve su ruta relativa.
/// Es `async` + `spawn_blocking` para que copiar un archivo grande no congele la interfaz.
#[tauri::command]
pub async fn importar_imagen(origen: String, state: State<'_, AppState>) -> Result<String, String> {
    let dir = state.almacen.dir().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || imagenes::importar(&dir, Path::new(&origen)))
        .await
        .map_err(|e| format!("La importación falló: {e}"))?
}

/// Registra una nueva entidad en el banco.
/// Calcula el siguiente ID disponible dentro del mismo lock para evitar colisiones
/// concurrentes y devuelve la entidad con su ID definitivo asignado.
#[tauri::command]
pub fn crear_entidad(mut entidad: Entidad, state: State<'_, AppState>) -> Entidad {
    let mut est = state.lock();
    let id = est.banco_entidades.iter().map(|e| e.id).max().unwrap_or(0) + 1;
    entidad.id = id;

    registrar(&mut est, &state.almacen, DeltaPartida::EntidadCreada(entidad.clone()));
    entidad
}

#[tauri::command]
pub fn modificar_entidad(entidad: Entidad, state: State<'_, AppState>) {
    {
        let est = state.lock();
        if let Some(anterior) = est.banco_entidades.iter().find(|e| e.id == entidad.id) {
            archivos::limpiar_desasociados(state.almacen.dir(), anterior, &entidad, &est.banco_entidades);
        }
    }
    procesar_delta(&state, DeltaPartida::EntidadModificada(entidad));
}

#[tauri::command]
pub fn borrar_entidad(entidad: Entidad, state: State<'_, AppState>) {
    {
        let est = state.lock();
        archivos::limpiar_todos_los_adjuntos(state.almacen.dir(), &entidad, &est.banco_entidades);
    }
    procesar_delta(&state, DeltaPartida::EntidadBorrada(entidad));
}

#[tauri::command]
pub async fn importar_archivo(
    adjunto: ArchivoAdjunto,
    origen: String,
    state: State<'_, AppState>,
) -> Result<ArchivoAdjunto, String> {
    let dir = state.almacen.dir().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        archivos::importar(&dir, adjunto.entidad_archivo, Path::new(&origen))
    })
    .await
    .map_err(|e| format!("Fallo al importar archivo: {e}"))?
}

#[tauri::command]
pub fn guardar_archivo_texto(
    adjunto: ArchivoAdjunto,
    contenido: String,
    state: State<'_, AppState>,
) -> Result<ArchivoAdjunto, String> {
    archivos::guardar_texto(state.almacen.dir(), adjunto, &contenido)
}

#[tauri::command]
pub fn leer_archivo_texto(ruta_relativa: String, state: State<'_, AppState>) -> Result<String, String> {
    archivos::leer_texto(state.almacen.dir(), &ruta_relativa)
}

#[tauri::command]
pub fn crear_documento_texto(
    entidad: Entidad,
    state: State<'_, AppState>,
) -> Result<ArchivoAdjunto, String> {
    archivos::crear_documento_texto(state.almacen.dir(), entidad)
}

#[tauri::command]
pub fn renombrar_archivo_adjunto(
    adjunto: ArchivoAdjunto,
    nuevo_nombre: String,
    state: State<'_, AppState>,
) -> Result<ArchivoAdjunto, String> {
    archivos::renombrar(state.almacen.dir(), adjunto, &nuevo_nombre)
}

#[tauri::command]
pub fn abrir_archivo_en_sistema(
    app: tauri::AppHandle,
    ruta_relativa: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let path = state.almacen.dir().join(&ruta_relativa);
    if !path.exists() {
        return Err(format!("El archivo no existe en: {}", path.display()));
    }

    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| format!("No se pudo abrir el archivo: {e}"))
}

/// Elimina de disco los archivos que se crearon en la ventana pero cuya edición se canceló
#[tauri::command]
pub fn descartar_archivos_borrador(
    rutas: Vec<String>,
    state: State<'_, AppState>,
) {
    let dir = state.almacen.dir();
    for ruta in rutas {
        let destino = dir.join(&ruta);
        if destino.is_file() {
            let _ = std::fs::remove_file(destino);
        }
    }
}