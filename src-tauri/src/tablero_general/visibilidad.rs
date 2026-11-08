// src-tauri/src/tablero_general/visibilidad.rs
//
// Qué ve cada uno de una instancia. Un parámetro es PÚBLICO si su `Parametro.publico` está activo
// (`normalizar_parametros` lo deja acorde a la visibilidad de su definición: siempre público,
// siempre privado, o lo que decida cada ficha). Lo que no es público no debe llegar a quien no sea
// el máster: ni su valor, ni su valor actual (la vida de una instancia), ni lo que se dibuje con él.
//
// `vista_jugador` es esa vista, calculada AQUÍ en Rust: la misma función filtrará lo que se envíe
// por red a los jugadores (// FUTURA RED).
use crate::tablero_general::modelos::Instancia;

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
                archivos_adjuntos: vec![Some("archivos/entidad_1/notas.txt".into())],
                es_unico: false,
            },
            valores_actuales: vec![ValorActual { id_definicion: ID_VIDA, valor: 7 }],
            coordenadas: Coordenada::Casilla(Casilla { x: 3, y: 4 }),
            activo: true,
            rotacion: 1,
            orden: 5,
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
}
