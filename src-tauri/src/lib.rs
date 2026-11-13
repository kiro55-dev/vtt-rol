// src-tauri/src/lib.rs
mod red;
mod tablero_general;

use tauri::{Emitter, Manager};

use red::comandos::{
    abrir_sala, asignar_propietario, cerrar_sala, comprobar_version_web, estado_sala, eliminar_jugador, expulsar_jugador, guardar_config_red, obtener_config_red,
    obtener_jugadores, poner_contrasena, quitar_propietario, regenerar_enlace,
};

use tablero_general::almacen::Almacen;
use tablero_general::dados::{limpiar_chat, tirar_dado};
use tablero_general::estado;
use tablero_general::partidas;
use tablero_general::partida::{
    corregir_tamanos_mapas, migrar_fondos_antiguos, cancelar_borrador_entidad_en,
    AppState, abrir_archivo_en_sistema, avisar_tablero, borrar_entidad, borrar_mapa, cancelar_borrador_entidad,
    borrar_definicion_parametro, modificar_definicion_parametro, crear_definicion_parametro, reordenar_definiciones_parametros,
    crear_documento_texto, crear_entidad, crear_mapa, crear_zona,
    descartar_archivos_borrador, eliminar_zona, guardar_archivo_texto, importar_archivo,
    importar_imagen, iniciar_autoguardado, iniciar_borrador_entidad, leer_archivo_texto,
    abrir_partida, crear_partida, obtener_partidas,
    modificar_entidad, modificar_mapa, modificar_zona, obtener_banco_entidades, obtener_definiciones_parametros,
    obtener_dir_datos,
    obtener_historial_personal, obtener_historial_tiradas, obtener_instancias_activas,
    instanciar_entidad, instanciar_entidades, borrar_entidades, restaurar_papelera, cerrar_partida, mover_instancia, mover_instancias, borrar_instancia, modificar_valor_actual, rotar_instancia, espejar_instancia,
    reordenar_instancias, compartir_mapa, abrir_mapa,
    obtener_mapas_zona, obtener_miniatura, guardar_miniatura_ficha, obtener_zonas, renombrar_archivo_adjunto,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Todo se guarda en el directorio de datos de la app, nunca en el directorio de trabajo.
            // Cada partida es una carpeta dentro de `<raiz>/partidas/`.
            let raiz = app.path().app_data_dir()?;
            std::fs::create_dir_all(&raiz)?;

            // Una sola partida suelta en la raíz (formato antiguo) pasa a ser «Partida 1»
            if partidas::migrar_formato_antiguo(&raiz)? {
                eprintln!("[partida] datos antiguos movidos a «{}»", partidas::PRIMERA);
            }
            let (nombre, recien_creada) = partidas::elegir_inicial(&raiz)?;
            partidas::guardar_ultima(&raiz, &nombre);
            let dir = partidas::ruta_partida(&raiz, &nombre);
            let almacen = Almacen::abrir(&dir)?;

            // Solo en desarrollo: trae los archivos que el sistema antiguo dejaba en src-tauri/.
            // Solo a una partida recién creada: en una existente (o una nueva que crees luego)
            // mezclaría datos viejos con los tuyos.
            #[cfg(debug_assertions)]
            {
                if let (true, Ok(cwd)) = (recien_creada, std::env::current_dir()) {
                    let copiados = almacen.migrar_desde(&cwd);
                    if !copiados.is_empty() {
                        eprintln!("[partida] migrados desde {}: {:?}", cwd.display(), copiados);
                    }
                }
            }
            #[cfg(not(debug_assertions))]
            let _ = recien_creada;

            let (mut estado_inicial, informe) = estado::cargar(&almacen)?;
            eprintln!("[partida] datos en {} · {:?}", dir.display(), informe);
            migrar_fondos_antiguos(&mut estado_inicial, &almacen);
            corregir_tamanos_mapas(&mut estado_inicial, &almacen);

            // Consolida los deltas pendientes en el snapshot al arrancar (mantiene corto el log).
            if informe.conviene_compactar() {
                if let Err(e) = estado::compactar(&almacen, &estado_inicial) {
                    eprintln!("[partida] {e}");
                }
            }

            let state = AppState::nuevo(estado_inicial, almacen, raiz);
            // La sala avisa al front de cada cambio (abrir, progreso, conectados...) y recibe los deltas
            let avisador = app.handle().clone();
            state.red.al_cambiar(move || {
                let _ = avisador.emit("sala-cambiada", ());
            });
            // Lo que el jugador pide al host se ejecuta con las mismas funciones que los comandos de Tauri, y la
            // pantalla del host se entera (relee, o pinta el ping de un jugador)
            let emisor = app.handle().clone();
            state.red.poner_emisor(move |evento, payload| {
                let _ = emisor.emit(evento, payload);
            });
            let (handle, red) = (app.handle().clone(), state.red.clone());
            state.red.poner_ejecutor(move |jugador, peticion| red::peticiones::ejecutar(&handle, &red, jugador, peticion));
            // Si un jugador se va con una ficha nueva a medias (sala cerrada, expulsado...), se anula su borrador
            let limpiador = app.handle().clone();
            state.red.poner_limpiador(move |ids| {
                let estado = limpiador.state::<AppState>();
                for id in ids {
                    cancelar_borrador_entidad_en(id, vec![], &estado);
                }
            });
            red::instalar(state.red.clone());
            app.manage(state);
            iniciar_autoguardado(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            // Cerrar la ventana (la única) cierra la aplicación: se vacía la papelera y se guarda.
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                cerrar_partida(&window.state::<AppState>());
            }
        })
        .invoke_handler(tauri::generate_handler![
            tirar_dado,
            limpiar_chat,
            obtener_zonas,
            obtener_definiciones_parametros,
            crear_definicion_parametro,
            borrar_definicion_parametro,
            modificar_definicion_parametro,
            reordenar_definiciones_parametros,
            obtener_partidas,
            crear_partida,
            abrir_partida,
            obtener_mapas_zona,
            obtener_banco_entidades,
            obtener_instancias_activas,
            instanciar_entidad,
            instanciar_entidades,
            mover_instancia,
            mover_instancias,
            borrar_instancia,
            modificar_valor_actual,
            rotar_instancia,
            espejar_instancia,
            reordenar_instancias,
            compartir_mapa,
            abrir_mapa,
            estado_sala,
            abrir_sala,
            cerrar_sala,
            obtener_config_red,
            guardar_config_red,
            comprobar_version_web,
            regenerar_enlace,
            poner_contrasena,
            expulsar_jugador,
            eliminar_jugador,
            obtener_jugadores,
            asignar_propietario,
            quitar_propietario,
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
            avisar_tablero,
            crear_entidad,
            borrar_entidad,
            borrar_entidades,
            restaurar_papelera,
            modificar_entidad,
            importar_archivo,
            guardar_archivo_texto,
            leer_archivo_texto,
            crear_documento_texto,
            renombrar_archivo_adjunto,
            abrir_archivo_en_sistema,
            descartar_archivos_borrador,
            iniciar_borrador_entidad,
            cancelar_borrador_entidad,
            obtener_miniatura,
            guardar_miniatura_ficha,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
