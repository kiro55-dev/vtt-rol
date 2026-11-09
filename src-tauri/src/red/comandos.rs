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
use crate::tablero_general::modelos::IdJugador;
use crate::tablero_general::partida::{self, AppState};

#[tauri::command]
pub fn estado_sala(state: State<'_, AppState>) -> EstadoSala {
    state.red.estado()
}

/// Abre la sala (puede tardar: la primera vez descarga cloudflared). El progreso llega por `sala-cambiada`.
#[tauri::command]
pub async fn abrir_sala(state: State<'_, AppState>) -> Result<EstadoSala, String> {
    let red = state.red.clone();
    red.abrir().await?;
    Ok(red.estado())
}

#[tauri::command]
pub fn cerrar_sala(state: State<'_, AppState>) {
    state.red.cerrar();
}

/// El enlace anterior deja de valer y todos tendrán que volver a poner la contraseña.
#[tauri::command]
pub fn regenerar_enlace(state: State<'_, AppState>) -> Result<(), String> {
    state.red.regenerar_enlace()
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

#[derive(Serialize)]
pub struct JugadorSala {
    pub id: IdJugador,
    pub nombre: String,
    pub conectado: bool,
    /// Las fichas del banco de las que es propietario.
    pub fichas: Vec<u16>,
}

/// Los jugadores de ESTA partida (los que entraron alguna vez), con si están conectados ahora.
#[tauri::command]
pub fn obtener_jugadores(state: State<'_, AppState>) -> Vec<JugadorSala> {
    let est = state.lock();
    let conectados = state.red.conectados(); // estado → red
    est.jugadores
        .iter()
        .map(|j| JugadorSala {
            id: j.id,
            nombre: j.nombre.clone(),
            conectado: conectados.contains(&j.id),
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
