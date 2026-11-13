// src-tauri/src/red/comandos.rs
//
// Comandos de Tauri de la ventana «Configuración de sala». Cada cambio avisa al front con el evento
// `sala-cambiada` (lo emite lib.rs a través de `Red::al_cambiar`), que vuelve a leer `estado_sala` y
// `obtener_jugadores`.
use serde::Serialize;
use tauri::State;

use crate::red::EstadoSala;
use crate::tablero_general::cambios_entidad::CambioEntidad;
use crate::tablero_general::estado::DeltaPartida;
use crate::tablero_general::jugadores;
use crate::tablero_general::modelos::{IdJugador, ID_HOST};
use crate::tablero_general::partida::{self, AppState};

#[tauri::command]
pub fn estado_sala(state: State<'_, AppState>) -> EstadoSala {
    let est = state.lock(); // estado → red
    state.red.estado(&est)
}

/// Abre la sala (puede tardar: la primera vez descarga cloudflared). El progreso llega por `sala-cambiada`.
#[tauri::command]
pub async fn abrir_sala(state: State<'_, AppState>) -> Result<EstadoSala, String> {
    let red = state.red.clone();
    red.abrir().await?;
    let est = state.lock();
    Ok(red.estado(&est))
}

/// Cierra la sala avisando a los jugadores (ven que el host la cerró) antes de parar el servidor y el túnel.
#[tauri::command]
pub async fn cerrar_sala(state: State<'_, AppState>) -> Result<(), String> {
    state.red.cerrar_avisando().await;
    Ok(())
}

/// ¿Los jugadores podrán entrar con la versión de esta app? Solo con Cloudflare (la página es la de GitHub Pages, que se
/// actualiza sola): se consulta su `version.json` y, si trae un protocolo más nuevo que el de la app, o una versión nueva,
/// se avisa. Sin red, o con otro modo de publicar, no hay aviso.
#[tauri::command]
pub async fn comprobar_version_web(state: State<'_, AppState>) -> Result<Option<crate::red::version::AvisoVersion>, String> {
    if !state.red.pagina_en_github() {
        return Ok(None);
    }
    Ok(crate::red::version::comprobar(crate::red::URL_CLIENTE).await)
}

/// Cómo se publica la sala en este equipo (`red.json`): túnel de Cloudflare o dirección propia.
#[tauri::command]
pub fn obtener_config_red(state: State<'_, AppState>) -> crate::red::config::ConfigRed {
    state.red.config()
}

/// Solo con la sala cerrada. Comprueba la dirección y el puerto antes de guardar.
#[tauri::command]
pub fn guardar_config_red(config: crate::red::config::ConfigRed, state: State<'_, AppState>) -> Result<(), String> {
    state.red.guardar_config(&config)
}

/// El enlace anterior deja de valer y todos tendrán que volver a poner la contraseña.
#[tauri::command]
pub async fn regenerar_enlace(state: State<'_, AppState>) -> Result<(), String> {
    state.red.regenerar_enlace()?;
    // Las etiquetas de los relés salen del secreto: la escucha se rehace con el enlace nuevo
    state.red.reiniciar_senal().await;
    Ok(())
}

/// `None` o vacía: sin contraseña.
#[tauri::command]
pub async fn poner_contrasena(contrasena: Option<String>, state: State<'_, AppState>) -> Result<(), String> {
    let red = state.red.clone();
    // Argon2 tarda un poco: fuera del hilo de la interfaz
    tauri::async_runtime::spawn_blocking(move || red.poner_contrasena(contrasena))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn expulsar_jugador(id: IdJugador, state: State<'_, AppState>) -> Result<(), String> {
    state.red.expulsar(id)
}

/// Quita a un jugador de la partida: lo echa de la sala si está (y olvida su navegador y sus fichas nuevas a medias) y
/// borra su registro. Sus fichas no se bloquean ni se borran: pasan a ser solo del host.
#[tauri::command]
pub fn eliminar_jugador(id: IdJugador, state: State<'_, AppState>) -> Result<(), String> {
    eliminar_jugador_en(&state, id)
}

pub fn eliminar_jugador_en(state: &AppState, id: IdJugador) -> Result<(), String> {
    if id == ID_HOST {
        return Err("El host no se puede quitar".into());
    }
    if !state.lock().jugadores.iter().any(|j| j.id == id) {
        return Err("Ese jugador no existe".into());
    }
    state.red.expulsar(id)?; // sin el estado cogido (orden de bloqueo: estado, luego la sala)
    state.almacen.borrar_historial_de(id); // sus comandos de dados (↑ ↓) se van con él
    {
        let mut est = state.lock();
        partida::registrar(&mut est, &state.almacen, DeltaPartida::JugadorEliminado(id));
    }
    state.red.avisar();
    Ok(())
}

#[derive(Serialize)]
pub struct JugadorSala {
    pub id: IdJugador,
    pub nombre: String,
    pub conectado: bool,
    /// Por dónde entró, si está conectado: para saber si la conexión directa funcionó o cayó al respaldo.
    pub via: Option<crate::red::transporte::Via>,
    /// Las fichas del banco de las que es propietario.
    pub fichas: Vec<u16>,
}

/// Los jugadores de ESTA partida (los que entraron alguna vez), con si están conectados ahora.
#[tauri::command]
pub fn obtener_jugadores(state: State<'_, AppState>) -> Vec<JugadorSala> {
    let est = state.lock();
    let conectados = state.red.conectados(); // estado → red
    let vias = state.red.vias();
    est.jugadores
        .iter()
        .map(|j| JugadorSala {
            id: j.id,
            nombre: j.nombre.clone(),
            conectado: conectados.contains(&j.id),
            via: vias.get(&j.id).copied(),
            fichas: est.banco_entidades.iter().filter(|e| e.propietarios.contains(&j.id)).map(|e| e.id).collect(),
        })
        .collect()
}

/// Da (o quita) a un jugador el control de una ficha: un `EntidadCambiada` solo con los propietarios.
fn cambiar_propietario(state: &AppState, id_entidad: u16, id_jugador: IdJugador, dar: bool) -> Result<(), String> {
    {
        let mut est = state.lock();
        let entidad = est.banco_entidades.iter().find(|e| e.id == id_entidad).ok_or("Esa ficha no existe")?;
        let mut propietarios = entidad.propietarios.clone();
        let tiene = propietarios.contains(&id_jugador);
        if dar == tiene {
            return Ok(());
        }
        if dar {
            propietarios.push(id_jugador);
        } else {
            propietarios.retain(|j| *j != id_jugador);
        }
        jugadores::validar_propietarios(&est, &propietarios)?;
        let delta = DeltaPartida::EntidadCambiada { id: id_entidad, cambios: vec![CambioEntidad::Propietarios(propietarios)] };
        partida::registrar(&mut est, &state.almacen, delta);
    }
    state.red.avisar();
    Ok(())
}

#[tauri::command]
pub fn asignar_propietario(id_entidad: u16, id_jugador: IdJugador, state: State<'_, AppState>) -> Result<(), String> {
    cambiar_propietario(&state, id_entidad, id_jugador, true)
}

#[tauri::command]
pub fn quitar_propietario(id_entidad: u16, id_jugador: IdJugador, state: State<'_, AppState>) -> Result<(), String> {
    cambiar_propietario(&state, id_entidad, id_jugador, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tablero_general::almacen::pruebas::dir_temporal;
    use crate::tablero_general::almacen::Almacen;
    use crate::tablero_general::estado::EstadoPartida;
    use crate::tablero_general::modelos::*;

    fn estado() -> AppState {
        let dir = dir_temporal("eliminar_jugador");
        AppState::nuevo(EstadoPartida::default(), Almacen::abrir(&dir).unwrap(), dir)
    }

    #[test]
    fn quitar_a_un_jugador_con_fichas_no_se_bloquea_y_las_deja_sin_dueno() {
        let state = estado();
        {
            let mut est = state.lock();
            let ficha = |id: u16, duenos: Vec<IdJugador>| Entidad {
                id, nombre: format!("F{id}"), tipo: TipoEntidad::Jugador, parametros: vec![], cuerpo: Cuerpo::Pequenio, zona_id: 1,
                foto_url: None, miniatura_url: None, foto_encuadre: Default::default(), archivos_adjuntos: vec![], es_unico: false,
                propietarios: duenos,
            };
            partida::registrar(&mut est, &state.almacen, DeltaPartida::JugadorRegistrado(Jugador { id: 1, nombre: "Ana".into() }));
            partida::registrar(&mut est, &state.almacen, DeltaPartida::EntidadCreada(ficha(10, vec![1])));
        }
        assert!(eliminar_jugador_en(&state, ID_HOST).is_err(), "el host no se quita");
        assert!(eliminar_jugador_en(&state, 5).is_err(), "no existe");

        eliminar_jugador_en(&state, 1).unwrap();
        let est = state.lock();
        assert!(est.jugadores.is_empty());
        assert!(est.banco_entidades[0].propietarios.is_empty(), "la ficha sigue, solo del host");
    }
}
