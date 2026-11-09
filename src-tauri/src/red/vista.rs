// src-tauri/src/red/vista.rs
//
// Lo que ve cada jugador de la partida, y qué ha cambiado desde lo último que se le envió. Es la única
// puerta de salida de datos hacia los jugadores: todo pasa por los filtros de visibilidad.rs.
//
// En vez de traducir cada delta a un mensaje para cada jugador, se calcula su vista entera tras cada cambio
// y se le envía la diferencia por id. Así es imposible que algo se filtre o se quede viejo cuando cambia la
// visibilidad: un parámetro que pasa a privado, una ficha que cambia de dueño, el mapa público que cambia...
// La vista de un jugador es pequeña (un mapa y sus fichas), así que calcularla es barato.
use serde::{Deserialize, Serialize};

use crate::tablero_general::estado::EstadoPartida;
use crate::tablero_general::modelos::{DefinicionParametro, Entidad, IdJugador, Instancia, Mapa};
use crate::tablero_general::visibilidad;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct VistaJugador {
    /// El mapa público (`None` = el host no enseña ninguno).
    pub mapa: Option<Mapa>,
    /// Instancias que ve: las de sus fichas (enteras) y las demás del mapa público (filtradas).
    pub instancias: Vec<Instancia>,
    /// Sus fichas del banco, enteras.
    pub entidades: Vec<Entidad>,
    /// Solo las definiciones de los parámetros que ve: los nombres de los privados no salen.
    pub definiciones: Vec<DefinicionParametro>,
}

/// El mapa nuevo, envuelto para distinguir «no cambia» (sin campo) de «ahora no hay ninguno» (`actual: null`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MapaActual {
    pub actual: Option<Mapa>,
}

/// Lo que cambió en la vista de un jugador. Lo que no aparece, sigue igual.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Cambios {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mapa: Option<MapaActual>,
    /// Nuevas o cambiadas (se sustituyen enteras por id).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instancias: Vec<Instancia>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instancias_quitadas: Vec<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entidades: Vec<Entidad>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entidades_quitadas: Vec<u16>,
    /// La lista entera cuando cambia (es corta).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definiciones: Option<Vec<DefinicionParametro>>,
}

impl Cambios {
    pub fn esta_vacio(&self) -> bool {
        *self == Cambios::default()
    }
}

/// La vista de `jugador` con la partida tal como está.
pub fn vista_para(jugador: IdJugador, est: &EstadoPartida) -> VistaJugador {
    let mapa = visibilidad::mapas_para(jugador, &est.mapas, est.mapa_publico).into_iter().find(|m| Some(m.id) == est.mapa_publico);
    let instancias = visibilidad::instancias_para(jugador, &est.instancias_activas, est.mapa_publico);
    let entidades = visibilidad::entidades_para(jugador, &est.banco_entidades);
    let usadas: Vec<u16> = instancias
        .iter()
        .map(|i| &i.base)
        .chain(&entidades)
        .flat_map(|e| e.parametros.iter().map(|p| p.id_definicion))
        .collect();
    let definiciones = est.definiciones_parametros.iter().filter(|d| usadas.contains(&d.id)).cloned().collect();
    VistaJugador { mapa, instancias, entidades, definiciones }
}

/// Qué hay que enviar para pasar de `antes` a `despues`. `None` si no cambió nada.
pub fn diferencias(antes: &VistaJugador, despues: &VistaJugador) -> Option<Cambios> {
    let mut c = Cambios::default();
    if antes.mapa != despues.mapa {
        c.mapa = Some(MapaActual { actual: despues.mapa.clone() });
    }
    (c.instancias, c.instancias_quitadas) =
        por_id(&antes.instancias, &despues.instancias, |i| i.id_instancia);
    (c.entidades, c.entidades_quitadas) = por_id(&antes.entidades, &despues.entidades, |e| e.id);
    if antes.definiciones != despues.definiciones {
        c.definiciones = Some(despues.definiciones.clone());
    }
    (!c.esta_vacio()).then_some(c)
}

/// Las nuevas o cambiadas de `despues`, y los ids de `antes` que ya no están.
fn por_id<T: Clone + PartialEq>(antes: &[T], despues: &[T], id: impl Fn(&T) -> u16) -> (Vec<T>, Vec<u16>) {
    let puestas = despues
        .iter()
        .filter(|d| antes.iter().find(|a| id(a) == id(d)) != Some(*d))
        .cloned()
        .collect();
    let quitadas = antes.iter().map(&id).filter(|i| !despues.iter().any(|d| id(d) == *i)).collect();
    (puestas, quitadas)
}

/// Aplica `cambios` a una vista (lo mismo que hace el cliente web). Para los tests: aplicar las diferencias
/// a la vista vieja debe dar la nueva.
#[cfg(test)]
pub fn aplicar(vista: &mut VistaJugador, cambios: &Cambios) {
    if let Some(m) = &cambios.mapa {
        vista.mapa = m.actual.clone();
    }
    fn poner<T: Clone>(lista: &mut Vec<T>, nuevas: &[T], quitadas: &[u16], id: impl Fn(&T) -> u16) {
        lista.retain(|x| !quitadas.contains(&id(x)));
        for n in nuevas {
            match lista.iter().position(|x| id(x) == id(n)) {
                Some(p) => lista[p] = n.clone(),
                None => lista.push(n.clone()),
            }
        }
    }
    poner(&mut vista.instancias, &cambios.instancias, &cambios.instancias_quitadas, |i| i.id_instancia);
    poner(&mut vista.entidades, &cambios.entidades, &cambios.entidades_quitadas, |e| e.id);
    if let Some(d) = &cambios.definiciones {
        vista.definiciones = d.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tablero_general::estado::DeltaPartida;
    use crate::tablero_general::modelos::*;
    use crate::tablero_general::papelera;

    fn mapa(id: u16) -> Mapa {
        Mapa {
            id, id_zona: 1, nombre: format!("M{id}"), url_imagen_fondo: Some(format!("imagenes/m{id}.png")), ancho: 1000,
            alto: 1000, rejilla: 50, color_opacidad: vec![255, 255, 255, 76], grosor: 1, orden: 0,
        }
    }

    fn ficha(id: u16, propietarios: Vec<IdJugador>) -> Entidad {
        let p = |id_definicion, n, publico| Parametro { id_definicion, valor: ValorParametro::Numero(n), publico };
        Entidad {
            id, nombre: format!("F{id}"), tipo: TipoEntidad::Enemigo,
            parametros: vec![p(ID_VIDA, 20, false), p(ID_ARMADURA, 12, true), p(ID_VELOCIDAD, 6, true), p(ID_RANGO_ARMA, 1, true)],
            cuerpo: Cuerpo::Pequenio, zona_id: 1, foto_url: None, miniatura_url: None, foto_encuadre: Default::default(),
            archivos_adjuntos: vec![Some(format!("archivos/entidad_{id}/notas.txt"))], es_unico: false, propietarios,
        }
    }

    fn instancia(id: u16, base: Entidad, id_mapa: Option<u16>) -> Instancia {
        Instancia {
            id_instancia: id, id_mapa, base,
            valores_actuales: vec![ValorActual { id_definicion: ID_VIDA, valor: 15 }],
            coordenadas: if id_mapa.is_some() { Coordenada::Casilla(Casilla { x: 1, y: 1 }) } else { Coordenada::NoPuesto },
            activo: true, rotacion: 0, espejo: false, orden: id, capa: 0,
        }
    }

    /// Mapas 1 (público) y 2; ficha 10 del jugador 1 (colocada en el 2), 11 solo del host en el 1, 12 solo
    /// del host sin instanciar.
    fn estado() -> EstadoPartida {
        let mut e = EstadoPartida::default();
        e.zonas = vec![Zona { id: 1, nombre: "Z".into(), orden: 0 }];
        e.jugadores = vec![Jugador { id: 1, nombre: "Ana".into() }];
        e.mapas = vec![mapa(1), mapa(2)];
        e.mapa_publico = Some(1);
        e.banco_entidades = vec![ficha(10, vec![1]), ficha(11, vec![]), ficha(12, vec![])];
        e.instancias_activas = vec![instancia(1, ficha(10, vec![1]), Some(2)), instancia(2, ficha(11, vec![]), Some(1))];
        e
    }

    #[test]
    fn un_jugador_solo_ve_el_mapa_publico_sus_fichas_y_lo_publico_de_las_demas() {
        let v = vista_para(1, &estado());
        assert_eq!(v.mapa.as_ref().map(|m| m.id), Some(1));
        assert_eq!(v.entidades.iter().map(|e| e.id).collect::<Vec<_>>(), vec![10], "la 11 y la 12 son del host");

        let suya = v.instancias.iter().find(|i| i.id_instancia == 1).expect("la suya aunque esté en otro mapa");
        assert_eq!(suya.base.parametros.len(), 4);
        assert_eq!(suya.base.archivos_adjuntos.len(), 1);

        let ajena = v.instancias.iter().find(|i| i.id_instancia == 2).expect("la del mapa público");
        assert!(ajena.base.parametros.iter().all(|p| p.publico) && ajena.valores_actuales.is_empty());
        assert!(ajena.base.archivos_adjuntos.is_empty());

        // Otro jugador sin fichas: solo la del mapa público, filtrada; y ninguna definición privada
        let otro = vista_para(7, &estado());
        assert_eq!(otro.instancias.iter().map(|i| i.id_instancia).collect::<Vec<_>>(), vec![2]);
        assert!(otro.entidades.is_empty());
        assert!(otro.definiciones.iter().all(|d| d.id != ID_VIDA), "ni el nombre de la vida privada");
    }

    #[test]
    fn sin_mapa_publico_solo_ve_sus_fichas() {
        let mut e = estado();
        e.aplicar_delta(&DeltaPartida::MapaPublicoCambiado(None));
        let v = vista_para(1, &e);
        assert!(v.mapa.is_none());
        assert_eq!(v.instancias.iter().map(|i| i.id_instancia).collect::<Vec<_>>(), vec![1]);
    }

    #[test]
    fn lo_que_esta_en_la_papelera_no_se_ve() {
        let mut e = estado();
        let d = papelera::borrar_mapa(&e, 1).unwrap();
        e.aplicar_delta(&d);
        let v = vista_para(7, &e);
        assert!(v.mapa.is_none() && v.instancias.is_empty());
    }

    #[test]
    fn las_diferencias_llevan_solo_lo_cambiado_y_aplicadas_dan_la_vista_nueva() {
        let mut e = estado();
        let antes = vista_para(7, &e);
        assert_eq!(diferencias(&antes, &antes), None);

        // Se mueve la del host: solo viaja esa instancia
        let mut movida = e.instancias_activas[1].clone();
        movida.coordenadas = Coordenada::Casilla(Casilla { x: 5, y: 5 });
        e.aplicar_delta(&DeltaPartida::InstanciaModificada(movida));
        let despues = vista_para(7, &e);
        let c = diferencias(&antes, &despues).unwrap();
        assert_eq!(c.instancias.len(), 1);
        assert!(c.mapa.is_none() && c.instancias_quitadas.is_empty() && c.definiciones.is_none());
        let mut aplicada = antes.clone();
        aplicar(&mut aplicada, &c);
        assert_eq!(aplicada, despues);

        // Cambia el mapa público: la instancia del 1 se va y el mapa cambia
        e.aplicar_delta(&DeltaPartida::MapaPublicoCambiado(Some(2)));
        let otra = vista_para(7, &e);
        let c = diferencias(&despues, &otra).unwrap();
        assert_eq!(c.mapa, Some(MapaActual { actual: Some(mapa(2)) }));
        assert_eq!(c.instancias_quitadas, vec![2]);
        let mut aplicada = despues.clone();
        aplicar(&mut aplicada, &c);
        assert_eq!(aplicada, otra);
    }

    #[test]
    fn un_parametro_que_pasa_a_privado_desaparece_del_jugador() {
        let mut e = estado();
        let antes = vista_para(7, &e);
        let mut sin_armadura = e.banco_entidades[1].clone();
        sin_armadura.parametros[1].publico = false; // la armadura de la 11
        let cambios = crate::tablero_general::cambios_entidad::entre(&e.banco_entidades[1], &sin_armadura);
        e.aplicar_delta(&DeltaPartida::EntidadCambiada { id: 11, cambios });

        let despues = vista_para(7, &e);
        let c = diferencias(&antes, &despues).unwrap();
        let inst = &c.instancias[0];
        assert!(inst.base.parametros.iter().all(|p| p.id_definicion != ID_ARMADURA), "ni su valor viejo");
        assert!(c.definiciones.unwrap().iter().all(|d| d.id != ID_ARMADURA), "ni su nombre");
    }

    #[test]
    fn al_recibir_una_ficha_le_llega_entera_y_al_perderla_se_le_quita() {
        let mut e = estado();
        let antes = vista_para(7, &e);
        e.jugadores.push(Jugador { id: 7, nombre: "Luis".into() });
        let cambios = vec![crate::tablero_general::cambios_entidad::CambioEntidad::Propietarios(vec![7])];
        e.aplicar_delta(&DeltaPartida::EntidadCambiada { id: 12, cambios });
        let despues = vista_para(7, &e);
        let c = diferencias(&antes, &despues).unwrap();
        assert_eq!(c.entidades.iter().map(|x| x.id).collect::<Vec<_>>(), vec![12]);
        assert_eq!(c.entidades[0].archivos_adjuntos.len(), 1);

        let cambios = vec![crate::tablero_general::cambios_entidad::CambioEntidad::Propietarios(vec![])];
        e.aplicar_delta(&DeltaPartida::EntidadCambiada { id: 12, cambios });
        let c = diferencias(&despues, &vista_para(7, &e)).unwrap();
        assert_eq!(c.entidades_quitadas, vec![12]);
    }
}
