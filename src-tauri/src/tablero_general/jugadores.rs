// src-tauri/src/tablero_general/jugadores.rs
//
// Jugadores de la partida y qué puede hacer cada uno. Preparado para la red (etapa 2): de momento solo
// juega el host, que lo puede todo, y nadie llama todavía a estas funciones fuera de los tests.
//
//  * El registro (`EstadoPartida.jugadores`) guarda un id por jugador: quien vuelve otro día conserva sus
//    fichas. El host es `ID_HOST` (0) y no está en la lista.
//  * Un jugador controla las fichas en cuyos `propietarios` está: las ve enteras, las edita (adjuntos .txt
//    incluidos), mueve sus instancias, las gira y las quita del tablero. Lo demás es del host.
//
// Como en instancias.rs, nada aquí cambia el estado: `registrar` devuelve el delta ya resuelto.
#![allow(dead_code)] // FUTURA RED: lo usarán los comandos que lleguen de otros jugadores

use crate::tablero_general::cambios_entidad::CambioEntidad;
use crate::tablero_general::estado::{DeltaPartida, EstadoPartida};
use crate::tablero_general::modelos::*;

pub const MAX_NOMBRE_JUGADOR: usize = 30;

/// Tamaño máximo de un texto (.txt adjunto) que mande un jugador por red.
pub const MAX_TEXTO_JUGADOR: usize = 256 * 1024;

/// Extensiones de adjunto que un jugador puede crear o subir. Solo texto: un .txt se muestra siempre como
/// texto y no puede ejecutar nada. Los PDF están prohibidos a propósito (pueden llevar JavaScript, acciones
/// que lanzan programas y exploits contra el lector) y no se depende de ningún visor.
pub const ADJUNTOS_DE_JUGADOR: [&str; 1] = ["txt"];

/// Da de alta a un jugador nuevo con el siguiente id libre (el 0 es del host).
pub fn registrar(est: &EstadoPartida, nombre: &str) -> Result<DeltaPartida, String> {
    let nombre = nombre.trim();
    if nombre.is_empty() {
        return Err("El jugador necesita un nombre".into());
    }
    if nombre.chars().count() > MAX_NOMBRE_JUGADOR {
        return Err(format!("El nombre del jugador no puede pasar de {MAX_NOMBRE_JUGADOR} caracteres"));
    }
    let id = est
        .jugadores
        .iter()
        .map(|j| j.id)
        .max()
        .unwrap_or(ID_HOST)
        .checked_add(1)
        .ok_or("No caben más jugadores")?;
    Ok(DeltaPartida::JugadorRegistrado(Jugador { id, nombre: nombre.to_string() }))
}

/// El host, o un jugador que está entre los propietarios.
pub fn es_propietario(jugador: IdJugador, entidad: &Entidad) -> bool {
    jugador == ID_HOST || entidad.propietarios.contains(&jugador)
}

/// Puede editar la ficha del banco `id_entidad`.
pub fn controla_entidad(est: &EstadoPartida, jugador: IdJugador, id_entidad: u16) -> bool {
    jugador == ID_HOST || est.banco_entidades.iter().any(|e| e.id == id_entidad && es_propietario(jugador, e))
}

/// Puede mover la instancia, girarla y quitarla del tablero.
pub fn controla_instancia(est: &EstadoPartida, jugador: IdJugador, id_instancia: u16) -> bool {
    jugador == ID_HOST
        || est
            .instancias_activas
            .iter()
            .any(|i| i.id_instancia == id_instancia && es_propietario(jugador, &i.base))
}

/// Qué cambios de su ficha puede hacer: el host todos; un propietario todos salvo los que organizan la
/// partida (de quién es, en qué zona está y si es única).
pub fn cambio_permitido(jugador: IdJugador, cambio: &CambioEntidad) -> bool {
    jugador == ID_HOST
        || !matches!(cambio, CambioEntidad::Propietarios(_) | CambioEntidad::Zona(_) | CambioEntidad::Unico(_))
}

/// Un jugador solo puede adjuntar archivos de texto (ver `ADJUNTOS_DE_JUGADOR`).
pub fn adjunto_permitido_a_jugador(ruta: &str) -> bool {
    std::path::Path::new(ruta)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ADJUNTOS_DE_JUGADOR.contains(&e.to_lowercase().as_str()))
}

/// Comprueba la lista de propietarios de una ficha: sin repetidos, sin el host (que no hace falta) y solo
/// jugadores registrados.
pub fn validar_propietarios(est: &EstadoPartida, propietarios: &[IdJugador]) -> Result<(), String> {
    for (i, id) in propietarios.iter().enumerate() {
        if *id == ID_HOST {
            return Err("El host no hace falta en los propietarios: ya lo controla todo".into());
        }
        if propietarios[..i].contains(id) {
            return Err("Un propietario está repetido".into());
        }
        if !est.jugadores.iter().any(|j| j.id == *id) {
            return Err(format!("El jugador {id} no existe"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ficha(id: u16, propietarios: Vec<IdJugador>) -> Entidad {
        Entidad {
            id,
            nombre: format!("F{id}"),
            tipo: TipoEntidad::Jugador,
            parametros: vec![],
            cuerpo: Cuerpo::Pequenio,
            zona_id: 1,
            foto_url: None,
            miniatura_url: None,
            foto_encuadre: Default::default(),
            archivos_adjuntos: vec![],
            es_unico: false,
            propietarios,
        }
    }

    fn estado() -> EstadoPartida {
        let mut e = EstadoPartida::default();
        e.jugadores = vec![Jugador { id: 1, nombre: "Ana".into() }, Jugador { id: 2, nombre: "Luis".into() }];
        e.banco_entidades = vec![ficha(10, vec![1]), ficha(11, vec![])];
        e.instancias_activas = vec![Instancia {
            id_instancia: 5,
            id_mapa: None,
            base: ficha(10, vec![1]),
            valores_actuales: vec![],
            coordenadas: Coordenada::NoPuesto,
            activo: true,
            rotacion: 0,
            espejo: false,
            orden: 0,
            capa: 0,
        }];
        e
    }

    #[test]
    fn los_ids_son_secuenciales_y_el_cero_es_del_host() {
        let mut e = EstadoPartida::default();
        for (esperado, nombre) in [(1, "Ana"), (2, " Luis "), (3, "Eva")] {
            let d = registrar(&e, nombre).unwrap();
            let DeltaPartida::JugadorRegistrado(j) = &d else { panic!("{d:?}") };
            assert_eq!(j.id, esperado);
            assert_eq!(j.nombre, nombre.trim());
            e.aplicar_delta(&d);
        }
        assert_eq!(e.jugadores.len(), 3);
        assert!(registrar(&e, "   ").is_err());
        assert!(registrar(&e, &"x".repeat(31)).is_err());
    }

    #[test]
    fn el_host_lo_controla_todo_y_un_jugador_solo_lo_suyo() {
        let e = estado();
        assert!(controla_entidad(&e, ID_HOST, 11));
        assert!(controla_entidad(&e, 1, 10));
        assert!(!controla_entidad(&e, 1, 11), "ficha solo del host");
        assert!(!controla_entidad(&e, 2, 10), "ficha de otro jugador");
        assert!(!controla_entidad(&e, 1, 99), "no existe");

        assert!(controla_instancia(&e, ID_HOST, 5));
        assert!(controla_instancia(&e, 1, 5));
        assert!(!controla_instancia(&e, 2, 5));
    }

    #[test]
    fn un_propietario_no_cambia_de_quien_es_ni_donde_esta_su_ficha() {
        assert!(cambio_permitido(1, &CambioEntidad::Nombre("X".into())));
        assert!(cambio_permitido(1, &CambioEntidad::Adjuntos(vec![])));
        for c in [CambioEntidad::Propietarios(vec![1, 2]), CambioEntidad::Zona(3), CambioEntidad::Unico(true)] {
            assert!(!cambio_permitido(1, &c), "{c:?}");
            assert!(cambio_permitido(ID_HOST, &c));
        }
    }

    #[test]
    fn un_jugador_solo_puede_adjuntar_texto() {
        assert!(adjunto_permitido_a_jugador("archivos/entidad_1/notas.txt"));
        assert!(adjunto_permitido_a_jugador("archivos/entidad_1/NOTAS.TXT"));
        for malo in ["a.pdf", "a.PDF", "a.exe", "a.svg", "a.html", "a.txt.js", "sin_extension"] {
            assert!(!adjunto_permitido_a_jugador(malo), "{malo}");
        }
    }

    #[test]
    fn los_propietarios_deben_ser_jugadores_registrados_sin_repetir_y_sin_el_host() {
        let e = estado();
        assert!(validar_propietarios(&e, &[]).is_ok());
        assert!(validar_propietarios(&e, &[1, 2]).is_ok());
        assert!(validar_propietarios(&e, &[ID_HOST]).is_err());
        assert!(validar_propietarios(&e, &[1, 1]).is_err());
        assert!(validar_propietarios(&e, &[7]).is_err());
    }
}
