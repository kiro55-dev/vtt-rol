// src-tauri/src/lib.rs
mod tablero_general;

use std::sync::Mutex;
use tauri::Manager;

use tablero_general::almacen::Almacen;
use tablero_general::dados::tirar_dado;
use tablero_general::estado;
use tablero_general::partida::{
    AppState, borrar_mapa, compactar_partida, crear_mapa, crear_zona, eliminar_zona, importar_imagen,
    iniciar_autoguardado, modificar_mapa, modificar_zona, obtener_banco_entidades,
    obtener_dir_datos, obtener_historial_personal, obtener_historial_tiradas,
    obtener_instancias_activas, obtener_mapas_zona, obtener_zonas, crear_entidad, borrar_entidad,
    modificar_entidad, importar_archivo, guardar_archivo_texto, leer_archivo_texto,
    crear_documento_texto, renombrar_archivo_adjunto,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Todo se guarda en el directorio de datos de la app, nunca en el directorio de trabajo.
            let dir = app.path().app_data_dir()?;
            let almacen = Almacen::abrir(&dir)?;

            // Solo en desarrollo: trae los archivos que el sistema antiguo dejaba en src-tauri/.
            #[cfg(debug_assertions)]
            {
                if let Ok(cwd) = std::env::current_dir() {
                    let copiados = almacen.migrar_desde(&cwd);
                    if !copiados.is_empty() {
                        eprintln!("[partida] migrados desde {}: {:?}", cwd.display(), copiados);
                    }
                }
            }

            let (estado_inicial, informe) = estado::cargar(&almacen)?;
            eprintln!("[partida] datos en {} · {:?}", dir.display(), informe);

            // Consolida los deltas pendientes en el snapshot al arrancar (mantiene corto el log).
            if informe.conviene_compactar() {
                if let Err(e) = estado::compactar(&almacen, &estado_inicial) {
                    eprintln!("[partida] {e}");
                }
            }

            app.manage(AppState {
                estado: Mutex::new(estado_inicial),
                almacen,
            });
            iniciar_autoguardado(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                let state = window.state::<AppState>();
                compactar_partida(&state);
            }
        })
        .invoke_handler(tauri::generate_handler![
            tirar_dado,
            obtener_zonas,
            obtener_mapas_zona,
            obtener_banco_entidades,
            obtener_instancias_activas,
            obtener_historial_tiradas,
            obtener_historial_personal,
            crear_zona,
            eliminar_zona,
            modificar_zona,
            crear_mapa,
            modificar_mapa,
            borrar_mapa,
            obtener_dir_datos,
            importar_imagen,
            crear_entidad, 
            borrar_entidad,
            modificar_entidad,
            importar_archivo, 
            guardar_archivo_texto, 
            leer_archivo_texto,
            crear_documento_texto,
            renombrar_archivo_adjunto,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
