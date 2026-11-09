// src-tauri/src/tablero_general/visibilidad.rs
//
// Qué ve cada uno de una instancia. Un parámetro es PÚBLICO si su `Parametro.publico` está activo
// (`normalizar_parametros` lo deja acorde a la visibilidad de su definición: siempre público,
// siempre privado, o lo que decida cada ficha). Lo que no es público no debe llegar a quien no sea
// el máster: ni su valor, ni su valor actual (la vida de una instancia), ni lo que se dibuje con él.
//
// `vista_jugador` es esa vista, calculada AQUÍ en Rust: la misma función filtrará lo que se envíe
// por red a los jugadores (// FUTURA RED).
//
// Por jugador (etapa 2): el host y los propietarios de una ficha la ven ENTERA (lo privado y los adjuntos
// incluidos); los demás solo ven sus instancias, filtradas con `vista_jugador`. Una ficha que no es suya
// nunca les llega como ficha del banco, así que la que solo es del host y no está instanciada no sale.
use crate::tablero_general::cambios_entidad::CambioEntidad;
use crate::tablero_general::estado::EstadoPartida;
use crate::tablero_general::jugadores::es_propietario;
use crate::tablero_general::modelos::{Entidad, IdJugador, Instancia, Mapa, Zona, ID_HOST};

/// Primera etapa: el máster transmite su pantalla por Discord, así que a él también se le oculta
/// lo privado (en «Fichas activas», en la caja de parámetros y en el tablero). Para que el máster
/// lo vea todo otra vez basta con ponerlo a `false`: sus datos completos siguen en la partida, y la
/// ventana de la ficha, donde se decide qué es público, siempre lo muestra todo.
pub const OCULTAR_PRIVADOS_AL_MASTER: bool = true;

/// La instancia tal como la ve un jugador: solo parámetros públicos (y solo el valor actual de
/// esos), y sin los archivos adjuntos de la ficha, que son notas del máster.
pub fn vista_jugador(inst: &Instancia) -> Instancia {
    let mut vista = inst.clone();
    vista.base.parametros.retain(|p| p.publico);
    let publicos: Vec<u16> = vista.base.parametros.iter().map(|p| p.id_definicion).collect();
    vista.valores_actuales.retain(|v| publicos.contains(&v.id_definicion));
    vista.base.archivos_adjuntos.clear();
    vista
}

/// Las instancias que muestra la pantalla del máster: todo, o solo lo público si
/// `ocultar_privados` (ver `OCULTAR_PRIVADOS_AL_MASTER`).
pub fn instancias_para_el_master(instancias: &[Instancia], ocultar_privados: bool) -> Vec<Instancia> {
    if ocultar_privados {
        instancias.iter().map(vista_jugador).collect()
    } else {
        instancias.to_vec()
    }
}

// ------------------------------------------------------------------
// Por jugador (FUTURA RED: lo que se envíe a cada uno pasará por aquí)
// ------------------------------------------------------------------

/// Las instancias tal como las ve `jugador`. El host, todas. Un jugador:
///  * las de sus fichas, enteras y estén donde estén (las tiene activas aunque no estén en el tablero);
///  * las demás, filtradas con `vista_jugador`, solo si están en el mapa público.
#[allow(dead_code)]
pub fn instancias_para(jugador: IdJugador, instancias: &[Instancia], mapa_publico: Option<u16>) -> Vec<Instancia> {
    instancias
        .iter()
        .filter_map(|i| {
            if es_propietario(jugador, &i.base) {
                Some(i.clone())
            } else if i.id_mapa.is_some() && i.id_mapa == mapa_publico {
                Some(vista_jugador(i))
            } else {
                None
            }
        })
        .collect()
}

/// Los mapas que recibe `jugador`: el host todos; un jugador solo el público.
#[allow(dead_code)]
pub fn mapas_para(jugador: IdJugador, mapas: &[Mapa], mapa_publico: Option<u16>) -> Vec<Mapa> {
    if jugador == ID_HOST {
        return mapas.to_vec();
    }
    mapas.iter().filter(|m| Some(m.id) == mapa_publico).cloned().collect()
}

/// Las zonas son organización del host: un jugador no recibe ninguna.
#[allow(dead_code)]
pub fn zonas_para(jugador: IdJugador, zonas: &[Zona]) -> Vec<Zona> {
    if jugador == ID_HOST { zonas.to_vec() } else { Vec::new() }
}

/// ¿Puede `jugador` pedir este archivo (ruta relativa de la partida)? El host sí. Un jugador solo si lo usa
/// algo que ve: el fondo del mapa público, la foto o miniatura de una instancia visible, o cualquier cosa de
/// sus propias fichas (adjuntos incluidos). Nunca la papelera ni los mapas y fichas ocultos.
#[allow(dead_code)]
pub fn recurso_visible_para(jugador: IdJugador, est: &EstadoPartida, ruta: &str) -> bool {
    if jugador == ID_HOST {
        return true;
    }
    let usa = |e: &Entidad, con_adjuntos: bool| {
        e.foto_url.as_deref() == Some(ruta)
            || e.miniatura_url.as_deref() == Some(ruta)
            || (con_adjuntos && e.archivos_adjuntos.iter().flatten().any(|a| a == ruta))
    };
    est.mapas.iter().any(|m| Some(m.id) == est.mapa_publico && m.url_imagen_fondo.as_deref() == Some(ruta))
        || est.banco_entidades.iter().any(|e| es_propietario(jugador, e) && usa(e, true))
        || instancias_para(jugador, &est.instancias_activas, est.mapa_publico)
            .iter()
            .any(|i| usa(&i.base, true))
}

/// Las fichas del banco que recibe `jugador`: el host todas; un jugador solo las suyas, enteras. Las de los
/// demás solo le llegan como instancia (`instancias_para`).
#[allow(dead_code)]
pub fn entidades_para(jugador: IdJugador, banco: &[Entidad]) -> Vec<Entidad> {
    if jugador == ID_HOST {
        return banco.to_vec();
    }
    banco.iter().filter(|e| es_propietario(jugador, e)).cloned().collect()
}

/// Los cambios de una ficha (`EntidadCambiada`) que recibe `jugador`. `entidad` es la ficha YA cambiada e
/// `instanciada` si tiene alguna instancia activa. `None` = no se le envía nada.
///  * Host o propietario: todos.
///  * Si no, solo si está instanciada (si no, ni sabe que existe), y sin adjuntos ni parámetros privados.
///
/// FUTURA RED: si un parámetro pasa de público a privado, o el jugador gana o pierde la ficha, un diff no
/// basta (se quedaría con el valor viejo, o no tendría la ficha): en esos casos el host le reenvía la
/// instancia o la ficha entera ya filtrada.
#[allow(dead_code)]
pub fn cambios_para(
    jugador: IdJugador,
    entidad: &Entidad,
    instanciada: bool,
    cambios: &[CambioEntidad],
) -> Option<Vec<CambioEntidad>> {
    if es_propietario(jugador, entidad) {
        return Some(cambios.to_vec());
    }
    if !instanciada {
        return None;
    }
    let visibles: Vec<CambioEntidad> = cambios
        .iter()
        .filter(|c| match c {
            CambioEntidad::Adjuntos(_) => false,
            CambioEntidad::Parametro(p) => p.publico,
            _ => true,
        })
        .cloned()
        .collect();
    (!visibles.is_empty()).then_some(visibles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tablero_general::modelos::*;

    fn instancia() -> Instancia {
        let param = |id, n, publico| Parametro { id_definicion: id, valor: ValorParametro::Numero(n), publico };
        Instancia {
            id_instancia: 1,
            id_mapa: Some(2),
            base: Entidad {
                id: 1,
                nombre: "Orco".into(),
                tipo: TipoEntidad::Enemigo,
                // La vida es privada; armadura y velocidad, públicas
                parametros: vec![param(ID_VIDA, 20, false), param(ID_ARMADURA, 12, true), param(ID_VELOCIDAD, 6, true)],
                cuerpo: Cuerpo::Pequenio,
                zona_id: 1,
                foto_url: None,
                miniatura_url: None,
                foto_encuadre: Default::default(),
                archivos_adjuntos: vec![Some("archivos/entidad_1/notas.txt".into())],
                es_unico: false,
                propietarios: vec![3],
            },
            valores_actuales: vec![ValorActual { id_definicion: ID_VIDA, valor: 7 }],
            coordenadas: Coordenada::Casilla(Casilla { x: 3, y: 4 }),
            activo: true,
            rotacion: 1,
            espejo: true,
            orden: 5,
            capa: 0,
        }
    }

    #[test]
    fn el_jugador_no_ve_lo_privado_ni_su_valor_actual_ni_las_notas() {
        let v = vista_jugador(&instancia());
        let ids: Vec<u16> = v.base.parametros.iter().map(|p| p.id_definicion).collect();
        assert_eq!(ids, vec![ID_ARMADURA, ID_VELOCIDAD]);
        assert!(v.valores_actuales.is_empty(), "la vida actual también es privada");
        assert!(v.base.archivos_adjuntos.is_empty());
        // Lo demás no cambia
        assert_eq!((v.id_instancia, v.id_mapa, v.rotacion, v.orden), (1, Some(2), 1, 5));
        assert!(v.espejo, "el aspecto no es privado");
        assert_eq!(v.coordenadas, Coordenada::Casilla(Casilla { x: 3, y: 4 }));
    }

    #[test]
    fn un_parametro_publico_comparte_tambien_su_valor_actual() {
        let mut i = instancia();
        i.base.parametros[0].publico = true; // la vida pasa a ser pública
        let v = vista_jugador(&i);
        assert_eq!(v.base.parametros.len(), 3);
        assert_eq!(v.valores_actuales, vec![ValorActual { id_definicion: ID_VIDA, valor: 7 }]);
    }

    #[test]
    fn la_vista_del_master_depende_del_interruptor_y_no_toca_los_datos() {
        let originales = vec![instancia()];
        let oculta = instancias_para_el_master(&originales, true);
        assert_eq!(oculta[0].base.parametros.len(), 2);
        let completa = instancias_para_el_master(&originales, false);
        assert_eq!(completa[0].base.parametros.len(), 3);
        assert_eq!(originales[0].base.parametros.len(), 3, "la partida conserva todo");
    }

    // La ficha de `instancia()` es del jugador 3

    // `instancia()` está en el mapa 2

    #[test]
    fn el_propietario_y_el_host_ven_la_instancia_entera_y_los_demas_filtrada() {
        let originales = vec![instancia()];
        for quien in [ID_HOST, 3] {
            let v = &instancias_para(quien, &originales, Some(2))[0];
            assert_eq!(v.base.parametros.len(), 3, "jugador {quien}");
            assert_eq!(v.valores_actuales.len(), 1, "con su vida actual");
            assert_eq!(v.base.archivos_adjuntos.len(), 1, "y sus adjuntos");
        }
        let ajena = &instancias_para(4, &originales, Some(2))[0];
        assert_eq!(ajena.base.parametros.len(), 2);
        assert!(ajena.valores_actuales.is_empty() && ajena.base.archivos_adjuntos.is_empty());
    }

    #[test]
    fn lo_que_no_esta_en_el_mapa_publico_no_se_envia_salvo_a_su_dueno() {
        let originales = vec![instancia()];
        assert!(instancias_para(4, &originales, Some(9)).is_empty(), "en otro mapa");
        assert!(instancias_para(4, &originales, None).is_empty(), "sin mapa público");
        assert_eq!(instancias_para(3, &originales, None).len(), 1, "el dueño siempre la tiene");
        assert_eq!(instancias_para(ID_HOST, &originales, None).len(), 1);

        // Sin colocar (activa fuera del tablero) tampoco sale a los demás aunque coincida el `None`
        let mut sin_colocar = instancia();
        sin_colocar.id_mapa = None;
        assert!(instancias_para(4, &[sin_colocar], None).is_empty());
    }

    fn mapa(id: u16, fondo: &str) -> Mapa {
        Mapa {
            id, id_zona: 1, nombre: id.to_string(), url_imagen_fondo: Some(fondo.into()), ancho: 1000, alto: 1000,
            rejilla: 50, color_opacidad: vec![255, 255, 255, 76], grosor: 1, orden: 0,
        }
    }

    #[test]
    fn un_jugador_solo_recibe_el_mapa_publico_y_ninguna_zona() {
        let mapas = vec![mapa(1, "imagenes/a.png"), mapa(2, "imagenes/b.png")];
        let ids = |j, p| mapas_para(j, &mapas, p).iter().map(|m| m.id).collect::<Vec<_>>();
        assert_eq!(ids(ID_HOST, Some(2)), vec![1, 2]);
        assert_eq!(ids(4, Some(2)), vec![2]);
        assert!(ids(4, None).is_empty());

        let zonas = vec![Zona { id: 1, nombre: "Z".into(), orden: 0 }];
        assert_eq!(zonas_para(ID_HOST, &zonas).len(), 1);
        assert!(zonas_para(4, &zonas).is_empty());
    }

    #[test]
    fn un_jugador_solo_puede_pedir_los_archivos_de_lo_que_ve() {
        let mut est = EstadoPartida::default();
        est.mapas = vec![mapa(1, "imagenes/oculto.png"), mapa(2, "imagenes/publico.png")];
        est.mapa_publico = Some(2);
        let mut i = instancia(); // mapa 2, ficha del jugador 3, con un adjunto
        i.base.foto_url = Some("imagenes/orco.png".into());
        est.instancias_activas = vec![i.clone()];
        est.banco_entidades = vec![i.base.clone()];
        // Una ficha oculta del host y lo que hay en la papelera
        let mut secreta = i.base.clone();
        secreta.id = 2;
        secreta.propietarios = vec![];
        secreta.foto_url = Some("imagenes/secreta.png".into());
        est.banco_entidades.push(secreta);

        assert!(recurso_visible_para(ID_HOST, &est, "imagenes/oculto.png"));
        assert!(recurso_visible_para(4, &est, "imagenes/publico.png"));
        assert!(!recurso_visible_para(4, &est, "imagenes/oculto.png"), "mapa no público");
        assert!(!recurso_visible_para(4, &est, "imagenes/secreta.png"), "ficha solo del host");
        assert!(recurso_visible_para(4, &est, "imagenes/orco.png"), "foto de una instancia visible");
        assert!(!recurso_visible_para(4, &est, "archivos/entidad_1/notas.txt"), "los adjuntos son del dueño");
        assert!(recurso_visible_para(3, &est, "archivos/entidad_1/notas.txt"), "el dueño sí");
        assert!(!recurso_visible_para(3, &est, "imagenes/secreta.png"));
    }

    #[test]
    fn un_jugador_solo_recibe_sus_fichas_del_banco() {
        let suya = instancia().base;
        let del_host = Entidad { id: 2, propietarios: vec![], ..suya.clone() };
        let de_otro = Entidad { id: 3, propietarios: vec![4], ..suya.clone() };
        let banco = vec![suya, del_host, de_otro];

        let ids = |j| entidades_para(j, &banco).iter().map(|e| e.id).collect::<Vec<_>>();
        assert_eq!(ids(ID_HOST), vec![1, 2, 3]);
        assert_eq!(ids(3), vec![1]);
        assert_eq!(ids(4), vec![3]);
        assert!(ids(9).is_empty());
        assert_eq!(entidades_para(3, &banco)[0].archivos_adjuntos.len(), 1, "la suya, entera");
    }

    #[test]
    fn los_cambios_de_una_ficha_ajena_solo_llegan_si_esta_instanciada_y_sin_lo_privado() {
        let ficha = instancia().base;
        let privado = Parametro { id_definicion: ID_VIDA, valor: ValorParametro::Numero(30), publico: false };
        let publico = Parametro { id_definicion: ID_ARMADURA, valor: ValorParametro::Numero(14), publico: true };
        let cambios = vec![
            CambioEntidad::Nombre("Orco jefe".into()),
            CambioEntidad::Parametro(privado),
            CambioEntidad::Parametro(publico.clone()),
            CambioEntidad::Adjuntos(vec![]),
        ];

        assert_eq!(cambios_para(3, &ficha, false, &cambios), Some(cambios.clone()), "el propietario, todo");
        assert_eq!(cambios_para(ID_HOST, &ficha, false, &cambios), Some(cambios.clone()));
        assert_eq!(cambios_para(4, &ficha, false, &cambios), None, "sin instancia no sabe que existe");
        assert_eq!(
            cambios_para(4, &ficha, true, &cambios),
            Some(vec![CambioEntidad::Nombre("Orco jefe".into()), CambioEntidad::Parametro(publico)])
        );
        assert_eq!(cambios_para(4, &ficha, true, &[CambioEntidad::Adjuntos(vec![])]), None, "nada que enviarle");
    }
}
