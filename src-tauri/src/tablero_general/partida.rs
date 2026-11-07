// src-tauri/src/tablero_general/partida.rs
//
// Comandos de Tauri y estado compartido. La lógica de estado/deltas está en `estado.rs`
// y el acceso a disco en `almacen.rs`.
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::tablero_general::almacen::{bloquear, Almacen};
use crate::tablero_general::archivos;
use crate::tablero_general::estado::{self, DeltaPartida, EstadoPartida};
use crate::tablero_general::imagenes;
use crate::tablero_general::miniaturas::{self, Tipo};
use crate::tablero_general::modelos::*;
use crate::tablero_general::recursos;

pub struct AppState {
    pub estado: Mutex<EstadoPartida>,
    pub almacen: Almacen,
    /// Ids reservados por fichas NUEVAS que se están editando y todavía no están en el banco.
    /// Orden de bloqueo: siempre `estado` primero y `borradores` después.
    borradores: Mutex<HashSet<u16>>,
}

impl AppState {
    pub fn nuevo(estado: EstadoPartida, almacen: Almacen) -> Self {
        Self { estado: Mutex::new(estado), almacen, borradores: Mutex::new(HashSet::new()) }
    }

    pub fn lock(&self) -> MutexGuard<'_, EstadoPartida> {
        bloquear(&self.estado)
    }

    fn reservas(&self) -> MutexGuard<'_, HashSet<u16>> {
        bloquear(&self.borradores)
    }
}

// ========================================================
// DESPACHO DE CAMBIOS
// ========================================================

/// Registra un delta con el lock YA cogido (para comandos que leen y escriben a la vez).
///
/// Si el cambio puede dejar sin uso imágenes o archivos (borrar un mapa o una ficha, cambiar una
/// foto, quitar un adjunto...), se comparan las referencias de antes y de después y se borra lo que
/// sobra, pero solo cuando el delta ya está escrito en el log: si algo falla, el cambio no se da
/// por hecho y no se borra nada.
fn registrar(est: &mut EstadoPartida, almacen: &Almacen, delta: DeltaPartida) {
    // FUTURA RED: aquí es donde enviarás `delta` por websocket/red.
    let antes = recursos::afecta_a_recursos(&delta).then(|| recursos::referencias(est));

    match est.registrar(almacen, delta) {
        Ok(()) => {
            if let Some(antes) = antes {
                let liberadas = recursos::liberar(almacen.dir(), &antes, &recursos::referencias(est));
                if !liberadas.is_empty() {
                    eprintln!("[recursos] liberados: {liberadas:?}");
                }
            }
        }
        Err(e) => eprintln!("[partida] {e}"),
    }
}

/// Punto de entrada para cualquier cambio: lo aplica en RAM y lo añade al log, en ese orden
/// y bajo el mismo lock, así el orden del log siempre coincide con el del estado.
pub fn procesar_delta(state: &AppState, delta: DeltaPartida) {
    let mut est = state.lock();
    registrar(&mut est, &state.almacen, delta);
}

// Función específica para registrar tiradas (llamada por dados.rs).
// Recibe el desglose y el total por separado: releer el total de un texto con `parse().unwrap_or(0)`
// guardaba un 0 silencioso cuando el número no cabía.
pub fn registrar_tirada(state: &AppState, desglose: &str, total: i32) {
    let tirada = Tiradas {
        total,
        comando: desglose.to_string(),
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

// ========================================================
// FICHAS (entidades)
// ========================================================

/// Una ficha NUEVA recibe su id real en cuanto se pulsa «Añadir ficha» (o se arrastra una imagen),
/// aunque todavía no exista en el banco. Así sus archivos viven desde el primer momento en su carpeta
/// definitiva `archivos/entidad_<id>/` y no hay carpeta «temp» compartida.
///  * Guardar  -> `crear_entidad`, que la añade al banco con ese mismo id.
///  * Cancelar -> `cancelar_borrador_entidad`, que borra la carpeta y la reserva.
#[tauri::command]
pub fn iniciar_borrador_entidad(
    zona_id: u16,
    foto_url: Option<String>,
    state: State<'_, AppState>,
) -> Result<Entidad, String> {
    let est = state.lock();
    let mut reservas = state.reservas();

    if !est.zonas.iter().any(|z| z.id == zona_id) {
        return Err("Crea primero una zona para poder añadir fichas".into());
    }
    if let Some(f) = foto_url.as_deref().filter(|f| !f.trim().is_empty()) {
        archivos::ruta_segura(state.almacen.dir(), f, &["imagenes"])?;
    }

    let id = recursos::siguiente_id_entidad(&est.banco_entidades, &reservas);
    reservas.insert(id);

    // Si quedó una carpeta de una ficha que nunca se guardó (cierre brusco), este id está libre
    // y esos archivos no son de nadie: se limpia aquí, en el único momento en que es seguro.
    archivos::borrar_carpeta_entidad(state.almacen.dir(), id);

    Ok(Entidad {
        id,
        nombre: "Nueva Ficha".into(),
        tipo: TipoEntidad::Enemigo,
        vida_max: 10,
        vida_es_publica: false,
        armadura: 10,
        armadura_es_publica: false,
        velocidad: 6,
        rango_arma: 1,
        cuerpo: Cuerpo::Pequenio,
        zona_id,
        foto_url,
        archivos_adjuntos: vec![],
        es_unico: false,
    })
}

/// Cancela una ficha nueva: libera su id, borra su carpeta y descarta lo creado en la sesión
/// (la foto, etc.) siempre que la partida no lo use.
#[tauri::command]
pub fn cancelar_borrador_entidad(id: u16, rutas: Vec<String>, state: State<'_, AppState>) {
    let est = state.lock();
    let mut reservas = state.reservas();

    if reservas.remove(&id) && !est.banco_entidades.iter().any(|e| e.id == id) {
        archivos::borrar_carpeta_entidad(state.almacen.dir(), id);
    }
    recursos::descartar(state.almacen.dir(), &est, &rutas);
}

/// Descarta archivos o imágenes creados durante una edición (al cancelar, o los que sobran al
/// guardar). Nunca borra nada que la partida esté usando.
#[tauri::command]
pub fn descartar_archivos_borrador(rutas: Vec<String>, state: State<'_, AppState>) {
    let est = state.lock();
    recursos::descartar(state.almacen.dir(), &est, &rutas);
}

fn validar_entidad(est: &EstadoPartida, dir: &Path, e: &Entidad) -> Result<(), String> {
    if e.nombre.trim().is_empty() {
        return Err("La ficha necesita un nombre".into());
    }
    for (campo, valor) in [
        ("La vida máxima", e.vida_max),
        ("La armadura", e.armadura),
        ("La velocidad", e.velocidad),
        ("El rango de arma", e.rango_arma),
    ] {
        if valor < 0 {
            return Err(format!("{campo} no puede ser negativa"));
        }
    }
    if let Cuerpo::Grande(dim) = &e.cuerpo {
        if dim.len() != 2 || dim.iter().any(|&d| d == 0) {
            return Err("El tamaño del cuerpo no es válido".into());
        }
    }
    if !est.zonas.iter().any(|z| z.id == e.zona_id) {
        return Err("La zona de la ficha ya no existe".into());
    }
    if let Some(f) = e.foto_url.as_deref().filter(|f| !f.trim().is_empty()) {
        archivos::ruta_segura(dir, f, &["imagenes"])?;
    }
    for a in e.archivos_adjuntos.iter().flatten() {
        archivos::ruta_segura(dir, a, &["archivos"])?;
    }
    Ok(())
}

/// Añade al banco una ficha iniciada con `iniciar_borrador_entidad` (su id ya es el definitivo).
#[tauri::command]
pub fn crear_entidad(entidad: Entidad, state: State<'_, AppState>) -> Result<Entidad, String> {
    let mut est = state.lock();
    let mut reservas = state.reservas();

    if !reservas.contains(&entidad.id) {
        return Err("Esta ficha no se inició con «Añadir ficha»".into());
    }
    if est.banco_entidades.iter().any(|e| e.id == entidad.id) {
        return Err("Esta ficha ya está guardada".into());
    }
    validar_entidad(&est, state.almacen.dir(), &entidad)?;

    reservas.remove(&entidad.id);
    registrar(&mut est, &state.almacen, DeltaPartida::EntidadCreada(entidad.clone()));
    Ok(entidad)
}

/// Guarda los cambios de una ficha. Los adjuntos que ya no están en la lista se borran solos (recursos.rs).
#[tauri::command]
pub fn modificar_entidad(entidad: Entidad, state: State<'_, AppState>) -> Result<(), String> {
    let mut est = state.lock();

    if !est.banco_entidades.iter().any(|e| e.id == entidad.id) {
        return Err("La ficha ya no existe (¿se borró su zona?)".into());
    }
    validar_entidad(&est, state.almacen.dir(), &entidad)?;

    registrar(&mut est, &state.almacen, DeltaPartida::EntidadModificada(entidad));
    Ok(())
}

/// Borra una ficha. Se usa la que está guardada (no la que manda el front), y sus adjuntos y su
/// foto se liberan solos si nadie más los usa.
#[tauri::command]
pub fn borrar_entidad(entidad: Entidad, state: State<'_, AppState>) -> Result<(), String> {
    let mut est = state.lock();

    let guardada = est
        .banco_entidades
        .iter()
        .find(|e| e.id == entidad.id)
        .cloned()
        .ok_or("La ficha ya no existe")?;

    registrar(&mut est, &state.almacen, DeltaPartida::EntidadBorrada(guardada));

    if !est.banco_entidades.iter().any(|e| e.id == entidad.id) {
        archivos::borrar_carpeta_entidad(state.almacen.dir(), entidad.id);
    }
    Ok(())
}

// ========================================================
// ARCHIVOS ADJUNTOS
// ========================================================

/// Solo se pueden adjuntar archivos a una ficha que existe o que se está creando.
fn comprobar_ficha_usable(state: &AppState, id: u16) -> Result<(), String> {
    let est = state.lock();
    let reservas = state.reservas();
    if est.banco_entidades.iter().any(|e| e.id == id) || reservas.contains(&id) {
        Ok(())
    } else {
        Err("La ficha no existe".into())
    }
}

#[tauri::command]
pub async fn importar_archivo(
    adjunto: ArchivoAdjunto,
    origen: String,
    state: State<'_, AppState>,
) -> Result<ArchivoAdjunto, String> {
    comprobar_ficha_usable(&state, adjunto.entidad_archivo.id)?;
    let dir = state.almacen.dir().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        archivos::importar(&dir, adjunto.entidad_archivo, Path::new(&origen))
    })
    .await
    .map_err(|e| format!("Fallo al importar archivo: {e}"))?
}

#[tauri::command]
pub fn crear_documento_texto(entidad: Entidad, state: State<'_, AppState>) -> Result<ArchivoAdjunto, String> {
    comprobar_ficha_usable(&state, entidad.id)?;
    archivos::crear_documento_texto(state.almacen.dir(), entidad)
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

/// Un archivo que la partida guardada ya usa se COPIA con el nombre nuevo (el original se libera
/// solo si se guarda la ficha, y no se toca si se cancela). Uno creado en esta edición se mueve.
#[tauri::command]
pub fn renombrar_archivo_adjunto(
    adjunto: ArchivoAdjunto,
    nuevo_nombre: String,
    state: State<'_, AppState>,
) -> Result<ArchivoAdjunto, String> {
    let copiar = {
        let est = state.lock();
        recursos::esta_referenciada(&est, &adjunto.ruta)
    };
    archivos::renombrar(state.almacen.dir(), adjunto, &nuevo_nombre, copiar)
}

/// Tipos que nunca se abren con el sistema desde la app (podrían ejecutar código).
const NO_ABRIR: [&str; 32] = [
    "exe", "bat", "cmd", "com", "scr", "msi", "msp", "ps1", "psm1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "hta",
    "cpl", "jar", "lnk", "url", "reg", "dll", "sh", "app", "appimage", "pif", "gadget", "msc", "scf", "apk", "bin", "run",
];

#[tauri::command]
pub fn abrir_archivo_en_sistema(
    app: tauri::AppHandle,
    ruta_relativa: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let path = archivos::ruta_segura(state.almacen.dir(), &ruta_relativa, &["archivos"])?;
    if !path.is_file() {
        return Err("El archivo no existe".into());
    }
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if NO_ABRIR.contains(&extension.as_str()) {
        return Err(format!("Por seguridad no se abren archivos .{extension} desde la aplicación"));
    }

    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| format!("No se pudo abrir el archivo: {e}"))
}

// ========================================================
// MINIATURAS
// ========================================================

/// Ruta relativa de la miniatura WebP de una imagen (se genera la primera vez), o `None` si esa
/// imagen no se puede reducir y hay que usar la original. `tipo`: "ficha" o "mapa".
#[tauri::command]
pub async fn obtener_miniatura(
    ruta: String,
    tipo: String,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let tipo = Tipo::desde_texto(&tipo).ok_or("Tipo de miniatura desconocido")?;
    let dir = state.almacen.dir().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || miniaturas::asegurar(&dir, &ruta, tipo))
        .await
        .map_err(|e| format!("No se pudo generar la miniatura: {e}"))?
}
