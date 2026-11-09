// src-tauri/src/red/vista.rs
//
// Lo que ve cada jugador de la partida, y qué ha cambiado desde lo último que se le envió. Es la única
// puerta de salida de datos hacia los jugadores: todo pasa por los filtros de visibilidad.rs.
//
// El jugador usa la MISMA app que el host (zonas, mapas, banco, «Fichas activas», tablero): su `invoke`
// contesta con esta vista (src/lib/red/replica.ts) en vez de con Rust. Por eso la vista tiene las mismas
// piezas que lee la app: zonas, mapas, banco, instancias y definiciones, pero solo lo que puede ver.
//
// En vez de traducir cada delta a un mensaje para cada jugador, se calcula su vista entera tras cada cambio
// y se le envía la diferencia por id. Así es imposible que algo se filtre o se quede viejo cuando cambia la
// visibilidad: un parámetro que pasa a privado, una ficha que cambia de dueño, un mapa que deja de compartirse...
use serde::{Deserialize, Serialize};

use crate::tablero_general::estado::EstadoPartida;
use crate::tablero_general::modelos::{DefinicionParametro, Entidad, IdJugador, Instancia, Mapa, Tiradas, Zona};
use crate::tablero_general::visibilidad;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct VistaJugador {
    /// Solo las zonas con algún mapa compartido.
    pub zonas: Vec<Zona>,
    /// Los mapas compartidos (la casilla de cada mapa en la lista del host).
    pub mapas: Vec<Mapa>,
    /// Instancias que ve: las de sus fichas (enteras) y las demás de los mapas compartidos (filtradas).
    pub instancias: Vec<Instancia>,
    /// Sus fichas del banco, enteras.
    pub entidades: Vec<Entidad>,
    /// Solo las definiciones de los parámetros que ve: los nombres de los privados no salen.
    pub definiciones: Vec<DefinicionParametro>,
    /// El chat de tiradas: es de todos (cada tirada lleva el nombre de quien la hizo), la más reciente primero.
    pub historial_tiradas: Vec<Tiradas>,
}

/// Lo que cambió en la vista de un jugador. Lo que no aparece, sigue igual.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Cambios {
    /// Nuevas o cambiadas (se sustituyen enteras por id).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub zonas: Vec<Zona>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub zonas_quitadas: Vec<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mapas: Vec<Mapa>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mapas_quitados: Vec<u16>,
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
    /// El chat entero cuando cambia (son 20 tiradas como mucho).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub historial_tiradas: Option<Vec<Tiradas>>,
}

impl Cambios {
    pub fn esta_vacio(&self) -> bool {
        *self == Cambios::default()
    }
}

/// La vista de `jugador` con la partida tal como está.
pub fn vista_para(jugador: IdJugador, est: &EstadoPartida) -> VistaJugador {
    let mapas = visibilidad::mapas_para(jugador, &est.mapas);
    let zonas = visibilidad::zonas_para(jugador, &est.zonas, &est.mapas);
    let compartidos: Vec<u16> = mapas.iter().map(|m| m.id).collect();
    let instancias = visibilidad::instancias_para(jugador, &est.instancias_activas, &compartidos);
    let entidades = visibilidad::entidades_para(jugador, &est.banco_entidades);
    let usadas: Vec<u16> = instancias
        .iter()
        .map(|i| &i.base)
        .chain(&entidades)
        .flat_map(|e| e.parametros.iter().map(|p| p.id_definicion))
        .collect();
    let definiciones = est.definiciones_parametros.iter().filter(|d| usadas.contains(&d.id)).cloned().collect();
    VistaJugador { zonas, mapas, instancias, entidades, definiciones, historial_tiradas: est.historial_tiradas.clone() }
}

/// Qué hay que enviar para pasar de `antes` a `despues`. `None` si no cambió nada.
pub fn diferencias(antes: &VistaJugador, despues: &VistaJugador) -> Option<Cambios> {
    let mut c = Cambios::default();
    (c.zonas, c.zonas_quitadas) = por_id(&antes.zonas, &despues.zonas, |z| z.id);
    (c.mapas, c.mapas_quitados) = por_id(&antes.mapas, &despues.mapas, |m| m.id);
    (c.instancias, c.instancias_quitadas) = por_id(&antes.instancias, &despues.instancias, |i| i.id_instancia);
    (c.entidades, c.entidades_quitadas) = por_id(&antes.entidades, &despues.entidades, |e| e.id);
    if antes.definiciones != despues.definiciones {
        c.definiciones = Some(despues.definiciones.clone());
    }
    if antes.historial_tiradas != despues.historial_tiradas {
        c.historial_tiradas = Some(despues.historial_tiradas.clone());
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

/// Aplica `cambios` a una vista (lo mismo que hace el navegador del jugador, src/lib/red/replica.ts). Para
/// los tests: aplicar las diferencias a la vista vieja debe dar la nueva.
#[cfg(test)]
pub fn aplicar(vista: &mut VistaJugador, cambios: &Cambios) {
    fn poner<T: Clone>(lista: &mut Vec<T>, nuevas: &[T], quitadas: &[u16], id: impl Fn(&T) -> u16) {
        lista.retain(|x| !quitadas.contains(&id(x)));
        for n in nuevas {
            match lista.iter().position(|x| id(x) == id(n)) {
                Some(p) => lista[p] = n.clone(),
                None => lista.push(n.clone()),
            }
        }
    }
    poner(&mut vista.zonas, &cambios.zonas, &cambios.zonas_quitadas, |z| z.id);
    poner(&mut vista.mapas, &cambios.mapas, &cambios.mapas_quitados, |m| m.id);
    poner(&mut vista.instancias, &cambios.instancias, &cambios.instancias_quitadas, |i| i.id_instancia);
    poner(&mut vista.entidades, &cambios.entidades, &cambios.entidades_quitadas, |e| e.id);
    if let Some(d) = &cambios.definiciones {
        vista.definiciones = d.clone();
    }
    if let Some(h) = &cambios.historial_tiradas {
        vista.historial_tiradas = h.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tablero_general::estado::DeltaPartida;
    use crate::tablero_general::modelos::*;
    use crate::tablero_general::papelera;

    fn mapa(id: u16, zona: u16, compartido: bool) -> Mapa {
        Mapa {
            id, id_zona: zona, nombre: format!("M{id}"), url_imagen_fondo: Some(format!("imagenes/m{id}.png")), ancho: 1000,
            alto: 1000, rejilla: 50, color_opacidad: vec![255, 255, 255, 76], grosor: 1, orden: 0, compartido,
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

    /// Zonas 1 y 2 (y la 3, vacía). Mapas 1 (zona 1, compartido), 2 (zona 1, no) y 3 (zona 2, no). Ficha 10
    /// del jugador 1 (colocada en el 2), 11 solo del host en el 1, 12 solo del host sin instanciar.
    fn estado() -> EstadoPartida {
        let mut e = EstadoPartida::default();
        e.zonas = (1..=3).map(|id| Zona { id, nombre: format!("Z{id}"), orden: 0 }).collect();
        e.jugadores = vec![Jugador { id: 1, nombre: "Ana".into() }];
        e.mapas = vec![mapa(1, 1, true), mapa(2, 1, false), mapa(3, 2, false)];
        e.banco_entidades = vec![ficha(10, vec![1]), ficha(11, vec![]), ficha(12, vec![])];
        e.instancias_activas = vec![instancia(1, ficha(10, vec![1]), Some(2)), instancia(2, ficha(11, vec![]), Some(1))];
        e
    }

    fn ids<T>(lista: &[T], id: impl Fn(&T) -> u16) -> Vec<u16> {
        lista.iter().map(id).collect()
    }

    #[test]
    fn un_jugador_ve_los_mapas_compartidos_sus_zonas_sus_fichas_y_lo_publico_de_las_demas() {
        let v = vista_para(1, &estado());
        assert_eq!(ids(&v.mapas, |m| m.id), vec![1]);
        assert_eq!(ids(&v.zonas, |z| z.id), vec![1], "solo la zona con un mapa compartido");
        assert_eq!(ids(&v.entidades, |e| e.id), vec![10], "la 11 y la 12 son del host");

        let suya = v.instancias.iter().find(|i| i.id_instancia == 1).expect("la suya aunque esté en otro mapa");
        assert_eq!(suya.base.parametros.len(), 4);
        assert_eq!(suya.base.archivos_adjuntos.len(), 1);

        let ajena = v.instancias.iter().find(|i| i.id_instancia == 2).expect("la del mapa compartido");
        assert!(ajena.base.parametros.iter().all(|p| p.publico) && ajena.valores_actuales.is_empty());
        assert!(ajena.base.archivos_adjuntos.is_empty());

        // Otro jugador sin fichas: solo la del mapa compartido, filtrada; y ninguna definición privada
        let otro = vista_para(7, &estado());
        assert_eq!(ids(&otro.instancias, |i| i.id_instancia), vec![2]);
        assert!(otro.entidades.is_empty());
        assert!(otro.definiciones.iter().all(|d| d.id != ID_VIDA), "ni el nombre de la vida privada");
    }

    #[test]
    fn sin_mapas_compartidos_solo_ve_sus_fichas() {
        let mut e = estado();
        e.aplicar_delta(&DeltaPartida::MapaCompartido { id: 1, compartido: false });
        let v = vista_para(1, &e);
        assert!(v.mapas.is_empty() && v.zonas.is_empty());
        assert_eq!(ids(&v.instancias, |i| i.id_instancia), vec![1]);
    }

    #[test]
    fn lo_que_esta_en_la_papelera_no_se_ve() {
        let mut e = estado();
        let d = papelera::borrar_mapa(&e, 1).unwrap();
        e.aplicar_delta(&d);
        let v = vista_para(7, &e);
        assert!(v.mapas.is_empty() && v.instancias.is_empty() && v.zonas.is_empty());
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
        assert!(c.mapas.is_empty() && c.zonas.is_empty() && c.instancias_quitadas.is_empty() && c.definiciones.is_none());
        let mut aplicada = antes.clone();
        aplicar(&mut aplicada, &c);
        assert_eq!(aplicada, despues);

        // Se comparte el 3 (otra zona) y se deja de compartir el 1: cambian mapas, zonas e instancias
        e.aplicar_delta(&DeltaPartida::MapaCompartido { id: 3, compartido: true });
        e.aplicar_delta(&DeltaPartida::MapaCompartido { id: 1, compartido: false });
        let otra = vista_para(7, &e);
        let c = diferencias(&despues, &otra).unwrap();
        assert_eq!(ids(&c.mapas, |m| m.id), vec![3]);
        assert_eq!(c.mapas_quitados, vec![1]);
        assert_eq!(ids(&c.zonas, |z| z.id), vec![2]);
        assert_eq!(c.zonas_quitadas, vec![1]);
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
    fn el_chat_es_de_todos_con_el_nombre_de_quien_tira_y_llega_como_cambio() {
        let mut e = estado();
        let antes = vista_para(7, &e);
        assert!(antes.historial_tiradas.is_empty());

        let tirada = |jugador, autor: &str| Tiradas { total: 9, comando: "1d10 (9)".into(), hora: "10:00".into(), jugador, autor: autor.into() };
        e.aplicar_delta(&DeltaPartida::TiradaRealizada(tirada(1, "Ana")));
        let despues = vista_para(7, &e);
        assert_eq!(despues.historial_tiradas[0].autor, "Ana", "un jugador ve lo que tira otro");
        let c = diferencias(&antes, &despues).unwrap();
        assert_eq!(c.historial_tiradas.as_ref().map(|h| h.len()), Some(1));
        assert!(c.instancias.is_empty() && c.mapas.is_empty());
        let mut aplicada = antes.clone();
        aplicar(&mut aplicada, &c);
        assert_eq!(aplicada, despues);

        // Y el host tira (sin autor): llega igual; limpiar el chat también
        e.aplicar_delta(&DeltaPartida::TiradaRealizada(tirada(0, "")));
        assert_eq!(vista_para(1, &e).historial_tiradas.len(), 2);
        e.aplicar_delta(&DeltaPartida::HistorialTiradasLimpiado);
        assert!(diferencias(&despues, &vista_para(7, &e)).unwrap().historial_tiradas.unwrap().is_empty());
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
        assert_eq!(ids(&c.entidades, |x| x.id), vec![12]);
        assert_eq!(c.entidades[0].archivos_adjuntos.len(), 1);

        let cambios = vec![crate::tablero_general::cambios_entidad::CambioEntidad::Propietarios(vec![])];
        e.aplicar_delta(&DeltaPartida::EntidadCambiada { id: 12, cambios });
        let c = diferencias(&despues, &vista_para(7, &e)).unwrap();
        assert_eq!(c.entidades_quitadas, vec![12]);
    }
}
