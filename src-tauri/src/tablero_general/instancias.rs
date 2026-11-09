// src-tauri/src/tablero_general/instancias.rs
//
// Reglas para crear, mover y quitar instancias (fichas activas / puestas en el tablero).
// Estas funciones NO cambian el estado: miran cómo está y devuelven el `DeltaPartida` que hay que
// registrar (o el motivo por el que no se puede). Así el delta ya lleva todo resuelto (id nuevo,
// casilla encajada...) y aplicarlo es determinista: lo mismo en disco que, en el futuro, en la red.
use serde::Deserialize;

use crate::tablero_general::estado::{DeltaPartida, EstadoPartida};
use crate::tablero_general::modelos::*;
use crate::tablero_general::papelera;
use crate::tablero_general::tablero;

/// Dónde se pone una instancia: una casilla de un mapa. `None` (en `Option<Destino>`) = activa pero
/// fuera del tablero.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Destino {
    pub id_mapa: u16,
    pub casilla: Casilla,
}

/// El menor id de instancia que no usa nadie (ni una activa ni una de la papelera).
fn id_instancia_libre(est: &EstadoPartida) -> Option<u16> {
    let mut usados: std::collections::HashSet<u16> = est.instancias_activas.iter().map(|i| i.id_instancia).collect();
    usados.extend(papelera::ids_instancias(est));
    (1..=u16::MAX).find(|id| !usados.contains(id))
}

/// Instancia una ficha del banco.
///  * Si es ÚNICA y ya tiene instancia, se reutiliza esa (mismo id y mismos valores actuales; solo
///    cambian el sitio y la copia de la ficha).
///  * Si no, nace una nueva con sus valores actuales al máximo.
pub fn instanciar(est: &EstadoPartida, id_entidad: u16, destino: Option<Destino>) -> Result<DeltaPartida, String> {
    let entidad = est.banco_entidades.iter().find(|e| e.id == id_entidad).ok_or("Esa ficha no existe")?;

    if entidad.es_unico {
        if let Some(existente) = est.instancias_activas.iter().find(|i| i.base.id == id_entidad) {
            // Conserva su giro (y por tanto sus lados) al cambiar de sitio
            let reutilizada = Instancia { base: entidad.clone(), activo: true, ..existente.clone() };
            let (id_mapa, coordenadas) = colocar(est, tablero::lados_instancia(&reutilizada), destino)?;
            let capa = capa_al_colocar(est, &reutilizada, destino);
            return Ok(DeltaPartida::InstanciaModificada(Instancia { id_mapa, coordenadas, capa, ..reutilizada }));
        }
    }
    let (id_mapa, coordenadas) = colocar(est, tablero::tamano_cuerpo(&entidad.cuerpo), destino)?;

    // Tampoco el de una instancia que está en la papelera (con su ficha, mapa o zona)
    let reservada = papelera::reservados(est).instancia;
    let id_instancia = match est.instancias_activas.iter().map(|i| i.id_instancia).chain([reservada]).max().filter(|&m| m > 0) {
        None => 1,
        Some(m) => match m.checked_add(1) {
            Some(id) => id,
            // Se llegó al último id: se reutiliza el menor libre (ni activo ni en la papelera), así nadie puede
            // dejar la partida sin ids creando y borrando instancias
            None => id_instancia_libre(est).ok_or("No caben más instancias")?,
        },
    };
    Ok(DeltaPartida::InstanciaCreada(Instancia {
        id_instancia,
        id_mapa,
        base: entidad.clone(),
        valores_actuales: valores_iniciales(&est.definiciones_parametros, entidad),
        coordenadas,
        activo: true,
        rotacion: 0,
        espejo: false,
        // La última de «Fichas activas»
        orden: est.instancias_activas.iter().map(|i| i.orden.saturating_add(1)).max().unwrap_or(0),
        // En el tablero, encima de todas; sin colocar no importa
        capa: if destino.is_some() { capa_superior(est) } else { 0 },
        atenuada: false,
    }))
}

/// Pone una instancia en otra casilla (o la quita del tablero con `None`); sigue activa.
/// La que se pone en el tablero queda encima de las demás.
pub fn mover(est: &EstadoPartida, id_instancia: u16, destino: Option<Destino>) -> Result<DeltaPartida, String> {
    let inst = buscar(est, id_instancia)?;
    let (id_mapa, coordenadas) = colocar(est, tablero::lados_instancia(inst), destino)?;
    let capa = capa_al_colocar(est, inst, destino);
    Ok(DeltaPartida::InstanciaModificada(Instancia { id_mapa, coordenadas, capa, ..inst.clone() }))
}

/// Mueve varias instancias del mismo mapa `(dx, dy)` casillas a la vez, conservando las distancias
/// entre ellas (`tablero::desplazar_grupo` las recorta en el borde). Devuelve un `InstanciaModificada`
/// por ficha, que se registran en orden. Quedan encima de las demás, sin cambiar cuál de ellas
/// está encima de cuál.
pub fn mover_grupo(est: &EstadoPartida, ids: &[u16], id_mapa: u16, dx: i32, dy: i32) -> Result<Vec<DeltaPartida>, String> {
    let mapa = est.mapas.iter().find(|m| m.id == id_mapa).ok_or("Ese mapa no existe")?;
    let mut grupo = Vec::with_capacity(ids.len());
    for id in ids {
        let inst = buscar(est, *id)?;
        let Coordenada::Casilla(casilla) = inst.coordenadas else { return Err("Esa ficha no está en el tablero".into()) };
        if inst.id_mapa != Some(id_mapa) {
            return Err("Esa ficha está en otro mapa".into());
        }
        if grupo.iter().any(|(i, _): &(&Instancia, _)| i.id_instancia == *id) {
            return Err("Una ficha repetida".into());
        }
        grupo.push((inst, casilla));
    }
    // Se reparten las capas de abajo arriba según estaban
    grupo.sort_by_key(|(i, _)| i.capa);
    let fichas: Vec<_> = grupo.iter().map(|(i, c)| (*c, tablero::lados_instancia(i))).collect();
    let destinos = tablero::desplazar_grupo(tablero::Rejilla::de(mapa), &fichas, dx, dy);
    let base = capa_superior(est);
    Ok(grupo
        .into_iter()
        .zip(destinos)
        .enumerate()
        .map(|(k, ((inst, _), casilla))| {
            DeltaPartida::InstanciaModificada(Instancia {
                coordenadas: Coordenada::Casilla(casilla),
                capa: base.saturating_add(k as u32),
                atenuada: false,
                ..inst.clone()
            })
        })
        .collect())
}

/// Capa para quedar encima de todas las fichas.
pub fn capa_superior(est: &EstadoPartida) -> u32 {
    est.instancias_activas.iter().map(|i| i.capa.saturating_add(1)).max().unwrap_or(0)
}

/// Al ponerla en el tablero sube encima de todas; al quitarla de él conserva la que tenía.
fn capa_al_colocar(est: &EstadoPartida, inst: &Instancia, destino: Option<Destino>) -> u32 {
    if destino.is_some() { capa_superior(est) } else { inst.capa }
}

/// Gira la instancia un cuarto de vuelta en el sentido del reloj, sobre su casilla central. Si al
/// girar ya no cabe en el mapa, se mete dentro.
pub fn rotar(est: &EstadoPartida, id_instancia: u16) -> Result<DeltaPartida, String> {
    let inst = buscar(est, id_instancia)?;
    let girada = Instancia { rotacion: (inst.rotacion + 1) % 4, ..inst.clone() };
    let destino = match (girada.id_mapa, girada.coordenadas) {
        (Some(id_mapa), Coordenada::Casilla(casilla)) => Some(Destino { id_mapa, casilla }),
        _ => None,
    };
    let (id_mapa, coordenadas) = colocar(est, tablero::lados_instancia(&girada), destino)?;
    Ok(DeltaPartida::InstanciaModificada(Instancia { id_mapa, coordenadas, ..girada }))
}

/// Pone o quita el espejo de la imagen de la instancia. Solo cambia el aspecto: ni lados ni casilla.
pub fn espejar(est: &EstadoPartida, id_instancia: u16) -> Result<DeltaPartida, String> {
    let inst = buscar(est, id_instancia)?;
    Ok(DeltaPartida::InstanciaModificada(Instancia { espejo: !inst.espejo, ..inst.clone() }))
}

/// Nuevo orden de «Fichas activas». Deben venir todas las instancias, cada una una vez.
pub fn reordenar(est: &EstadoPartida, ids: Vec<u16>) -> Result<DeltaPartida, String> {
    let mut actuales: Vec<u16> = est.instancias_activas.iter().map(|i| i.id_instancia).collect();
    let mut pedidos = ids.clone();
    actuales.sort_unstable();
    pedidos.sort_unstable();
    if actuales != pedidos {
        return Err("El orden no coincide con las fichas activas".into());
    }
    Ok(DeltaPartida::InstanciasReordenadas(ids))
}

/// La instancia deja de estar activa.
pub fn borrar(est: &EstadoPartida, id_instancia: u16) -> Result<DeltaPartida, String> {
    buscar(est, id_instancia).map(|i| DeltaPartida::InstanciaBorrada(i.id_instancia))
}

/// Tope (en positivo y en negativo) de un valor actual, para que sumar deltas nunca desborde un i16.
pub const LIMITE_VALOR_ACTUAL: i16 = 16_000;

/// Cambia el valor actual de un parámetro con máximo. El máximo es solo el valor por defecto: el
/// actual puede pasarse de él (vida extra) o bajar de cero (vida negativa), dentro de ±`LIMITE_VALOR_ACTUAL`.
pub fn cambiar_valor_actual(
    est: &EstadoPartida,
    id_instancia: u16,
    id_definicion: u16,
    delta: i16,
) -> Result<Option<DeltaPartida>, String> {
    let inst = buscar(est, id_instancia)?;
    let actual = inst
        .valores_actuales
        .iter()
        .find(|v| v.id_definicion == id_definicion)
        .ok_or("Ese parámetro no tiene valor actual")?
        .valor;
    if !matches!(
        inst.base.parametros.iter().find(|p| p.id_definicion == id_definicion).map(|p| &p.valor),
        Some(ValorParametro::Numero(_))
    ) {
        return Err("Ese parámetro no tiene máximo".into());
    }
    let nuevo = actual.saturating_add(delta).clamp(-LIMITE_VALOR_ACTUAL, LIMITE_VALOR_ACTUAL);
    // Sin cambio (ya estaba en el tope): no se registra nada
    Ok((nuevo != actual).then(|| DeltaPartida::ValorActualModificado { id_instancia, id_definicion, delta: nuevo - actual }))
}

fn buscar(est: &EstadoPartida, id_instancia: u16) -> Result<&Instancia, String> {
    est.instancias_activas.iter().find(|i| i.id_instancia == id_instancia).ok_or_else(|| "Esa instancia no existe".into())
}

/// Mapa y casilla finales: la casilla se encaja para que el cuerpo entero quepa en el mapa.
fn colocar(est: &EstadoPartida, lados: (u16, u16), destino: Option<Destino>) -> Result<(Option<u16>, Coordenada), String> {
    let Some(d) = destino else { return Ok((None, Coordenada::NoPuesto)) };
    let mapa = est.mapas.iter().find(|m| m.id == d.id_mapa).ok_or("Ese mapa no existe")?;
    let casilla = tablero::encajar(tablero::Rejilla::de(mapa), d.casilla, lados);
    Ok((Some(mapa.id), Coordenada::Casilla(casilla)))
}

/// Cada parámetro con máximo empieza en el valor de la ficha (que es el máximo).
pub fn valores_iniciales(defs: &[DefinicionParametro], entidad: &Entidad) -> Vec<ValorActual> {
    defs.iter()
        .filter(|d| d.tiene_maximo)
        .filter_map(|d| match entidad.parametros.iter().find(|p| p.id_definicion == d.id)?.valor {
            ValorParametro::Numero(n) => Some(ValorActual { id_definicion: d.id, valor: n }),
            ValorParametro::Texto(_) => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapa(id: u16) -> Mapa {
        Mapa {
            id,
            id_zona: 1,
            nombre: id.to_string(),
            url_imagen_fondo: None,
            ancho: 1000,
            alto: 500,
            rejilla: 50, // 20×10 casillas
            color_opacidad: vec![255, 255, 255, 76],
            grosor: 1,
            orden: 0,
            compartido: false,
        }
    }

    fn entidad(id: u16, es_unico: bool, cuerpo: Cuerpo) -> Entidad {
        Entidad {
            id,
            nombre: format!("Ficha {id}"),
            tipo: TipoEntidad::Enemigo,
            parametros: vec![
                Parametro { id_definicion: ID_VIDA, valor: ValorParametro::Numero(30), publico: true },
                Parametro { id_definicion: ID_ARMADURA, valor: ValorParametro::Numero(12), publico: true },
            ],
            cuerpo,
            zona_id: 1,
            foto_url: None,
            miniatura_url: None,
            foto_encuadre: Default::default(),
            archivos_adjuntos: vec![],
            es_unico,
            propietarios: vec![],
        }
    }

    fn estado() -> EstadoPartida {
        let mut e = EstadoPartida::default();
        e.mapas = vec![mapa(1), mapa(2)];
        e.banco_entidades = vec![
            entidad(1, false, Cuerpo::Pequenio),
            entidad(2, true, Cuerpo::Pequenio),
            entidad(3, false, Cuerpo::Grande(vec![2, 3])),
        ];
        e
    }

    const fn en(id_mapa: u16, x: u16, y: u16) -> Option<Destino> {
        Some(Destino { id_mapa, casilla: Casilla { x, y } })
    }

    /// Construye el delta y lo aplica, como hace el comando.
    fn hacer(e: &mut EstadoPartida, construir: impl FnOnce(&EstadoPartida) -> Result<DeltaPartida, String>) -> DeltaPartida {
        let d = construir(e).unwrap();
        e.aplicar_delta(&d);
        d
    }

    #[test]
    fn al_llegar_al_ultimo_id_se_reutiliza_el_menor_libre() {
        let mut e = estado();
        hacer(&mut e, |e| instanciar(e, 1, None));
        let DeltaPartida::InstanciaCreada(mut ultima) = instanciar(&e, 1, None).unwrap() else { panic!() };
        ultima.id_instancia = u16::MAX;
        e.aplicar_delta(&DeltaPartida::InstanciaCreada(ultima));
        let DeltaPartida::InstanciaCreada(nueva) = instanciar(&e, 1, None).unwrap() else { panic!("debía crearla") };
        assert_eq!(nueva.id_instancia, 2, "el 1 está ocupado y el 65535 también");
    }

    #[test]
    fn instanciar_crea_con_id_nuevo_y_valores_al_maximo() {
        let mut e = estado();
        let d = hacer(&mut e, |e| instanciar(e, 1, en(1, 4, 5)));
        assert!(matches!(d, DeltaPartida::InstanciaCreada(_)));

        let i = &e.instancias_activas[0];
        assert_eq!(i.id_instancia, 1);
        assert_eq!((i.id_mapa, i.coordenadas), (Some(1), Coordenada::Casilla(Casilla { x: 4, y: 5 })));
        // Solo la vida tiene máximo: empieza en el de la ficha
        assert_eq!(i.valores_actuales, vec![ValorActual { id_definicion: ID_VIDA, valor: 30 }]);
        assert!(i.activo);

        // Una ficha no única puede tener varias instancias
        hacer(&mut e, |e| instanciar(e, 1, en(1, 0, 0)));
        assert_eq!(e.instancias_activas.iter().map(|i| i.id_instancia).collect::<Vec<_>>(), vec![1, 2]);
    }

    #[test]
    fn instanciar_sin_destino_la_deja_activa_fuera_del_tablero() {
        let mut e = estado();
        hacer(&mut e, |e| instanciar(e, 1, None));
        let i = &e.instancias_activas[0];
        assert_eq!((i.id_mapa, i.coordenadas), (None, Coordenada::NoPuesto));
    }

    #[test]
    fn una_ficha_unica_se_reutiliza_conservando_sus_valores() {
        let mut e = estado();
        hacer(&mut e, |e| instanciar(e, 2, en(1, 3, 3)));
        hacer(&mut e, |e| cambiar_valor_actual(e, 1, ID_VIDA, -12).map(Option::unwrap));

        // Arrastrada otra vez desde el banco, a otro mapa
        let d = hacer(&mut e, |e| instanciar(e, 2, en(2, 7, 1)));
        assert!(matches!(d, DeltaPartida::InstanciaModificada(_)));
        assert_eq!(e.instancias_activas.len(), 1);
        let i = &e.instancias_activas[0];
        assert_eq!(i.id_instancia, 1);
        assert_eq!((i.id_mapa, i.coordenadas), (Some(2), Coordenada::Casilla(Casilla { x: 7, y: 1 })));
        assert_eq!(i.valores_actuales[0].valor, 18, "conserva la vida que tenía");
    }

    #[test]
    fn el_destino_se_encaja_con_el_cuerpo_entero() {
        let mut e = estado();
        // 2×3 en la esquina inferior derecha (20×10): el centro queda en (18, 8)
        hacer(&mut e, |e| instanciar(e, 3, en(1, 19, 9)));
        assert_eq!(e.instancias_activas[0].coordenadas, Coordenada::Casilla(Casilla { x: 18, y: 8 }));
        // Y en la esquina superior izquierda, en (0, 1)
        hacer(&mut e, |e| mover(e, 1, en(1, 0, 0)));
        assert_eq!(e.instancias_activas[0].coordenadas, Coordenada::Casilla(Casilla { x: 0, y: 1 }));
    }

    #[test]
    fn errores_de_ficha_mapa_o_instancia_inexistentes() {
        let e = estado();
        assert!(instanciar(&e, 99, None).is_err());
        assert!(instanciar(&e, 1, en(99, 0, 0)).is_err());
        assert!(mover(&e, 5, None).is_err());
        assert!(borrar(&e, 5).is_err());
        assert!(cambiar_valor_actual(&e, 5, ID_VIDA, 1).is_err());
    }

    #[test]
    fn mover_quitar_del_tablero_y_borrar() {
        let mut e = estado();
        hacer(&mut e, |e| instanciar(e, 1, None));
        hacer(&mut e, |e| instanciar(e, 1, None));

        hacer(&mut e, |e| mover(e, 2, en(2, 5, 5)));
        assert_eq!(e.instancias_activas[1].id_mapa, Some(2));

        hacer(&mut e, |e| mover(e, 2, None));
        let i = &e.instancias_activas[1];
        assert_eq!((i.id_mapa, i.coordenadas, i.activo), (None, Coordenada::NoPuesto, true));

        hacer(&mut e, |e| borrar(e, 1));
        assert_eq!(e.instancias_activas.iter().map(|i| i.id_instancia).collect::<Vec<_>>(), vec![2]);
        // El id de una instancia borrada no se reutiliza mientras haya una mayor
        hacer(&mut e, |e| instanciar(e, 1, None));
        assert_eq!(e.instancias_activas.last().unwrap().id_instancia, 3);
    }

    #[test]
    fn rotar_intercambia_los_lados_y_reencaja_junto_al_borde() {
        let mut e = estado();
        // 2×3 pegado a la derecha (20×10): centro (18, 5), ocupa x 18..19
        hacer(&mut e, |e| instanciar(e, 3, en(1, 19, 5)));
        assert_eq!(tablero::lados_instancia(&e.instancias_activas[0]), (2, 3));

        // Girado es 3×2: ocuparía x 17..19 con el mismo centro, cabe
        hacer(&mut e, |e| rotar(e, 1));
        let i = &e.instancias_activas[0];
        assert_eq!((i.rotacion, tablero::lados_instancia(i)), (1, (3, 2)));
        assert_eq!(i.coordenadas, Coordenada::Casilla(Casilla { x: 18, y: 5 }));

        // Pegado arriba del todo y girado otra vez a 2×3: no cabe con y=0 y baja una
        hacer(&mut e, |e| mover(e, 1, en(1, 5, 0)));
        hacer(&mut e, |e| rotar(e, 1));
        assert_eq!(e.instancias_activas[0].coordenadas, Coordenada::Casilla(Casilla { x: 5, y: 1 }));

        // Cuatro giros vuelven al principio
        hacer(&mut e, |e| rotar(e, 1));
        hacer(&mut e, |e| rotar(e, 1));
        assert_eq!(e.instancias_activas[0].rotacion, 0);

        // Sin colocar también gira
        hacer(&mut e, |e| mover(e, 1, None));
        hacer(&mut e, |e| rotar(e, 1));
        assert_eq!(e.instancias_activas[0].coordenadas, Coordenada::NoPuesto);
    }

    #[test]
    fn espejar_solo_cambia_el_aspecto_y_se_puede_deshacer_con_otro_espejo() {
        let mut e = estado();
        hacer(&mut e, |e| instanciar(e, 3, en(1, 5, 5)));
        let antes = e.instancias_activas[0].clone();
        assert!(!antes.espejo);

        hacer(&mut e, |e| espejar(e, 1));
        let i = &e.instancias_activas[0];
        assert!(i.espejo);
        assert_eq!((i.coordenadas, i.rotacion, i.capa), (antes.coordenadas, antes.rotacion, antes.capa));
        assert_eq!(tablero::lados_instancia(i), tablero::lados_instancia(&antes));

        // Gira y sigue en espejo; otro espejo lo quita
        hacer(&mut e, |e| rotar(e, 1));
        assert!(e.instancias_activas[0].espejo);
        hacer(&mut e, |e| espejar(e, 1));
        assert!(!e.instancias_activas[0].espejo);
        assert!(espejar(&e, 99).is_err());

        // Una partida antigua no lo trae: sin espejo
        let vieja: Instancia = serde_json::from_value({
            let mut v = serde_json::to_value(&e.instancias_activas[0]).unwrap();
            v.as_object_mut().unwrap().remove("espejo");
            v
        })
        .unwrap();
        assert!(!vieja.espejo);
    }

    #[test]
    fn una_unica_reutilizada_conserva_su_giro() {
        let mut e = estado();
        e.banco_entidades[1].cuerpo = Cuerpo::Grande(vec![1, 3]);
        hacer(&mut e, |e| instanciar(e, 2, en(1, 5, 5)));
        hacer(&mut e, |e| rotar(e, 1));
        hacer(&mut e, |e| instanciar(e, 2, en(2, 0, 0)));
        let i = &e.instancias_activas[0];
        assert_eq!(i.rotacion, 1);
        // 3×1 en la esquina: el centro en x=1
        assert_eq!(i.coordenadas, Coordenada::Casilla(Casilla { x: 1, y: 0 }));
    }

    #[test]
    fn el_orden_de_las_fichas_activas() {
        let mut e = estado();
        for _ in 0..3 {
            hacer(&mut e, |e| instanciar(e, 1, None));
        }
        assert_eq!(e.instancias_activas.iter().map(|i| i.orden).collect::<Vec<_>>(), vec![0, 1, 2]);

        hacer(&mut e, |e| reordenar(e, vec![3, 1, 2]));
        let orden = |e: &EstadoPartida, id: u16| e.instancias_activas.iter().find(|i| i.id_instancia == id).unwrap().orden;
        assert_eq!((orden(&e, 3), orden(&e, 1), orden(&e, 2)), (0, 1, 2));

        // Una nueva va al final
        hacer(&mut e, |e| instanciar(e, 1, None));
        assert_eq!(orden(&e, 4), 3);

        // Faltan o sobran ids: no
        assert!(reordenar(&e, vec![1, 2, 3]).is_err());
        assert!(reordenar(&e, vec![1, 2, 3, 4, 4]).is_err());
    }

    #[test]
    fn una_instancia_guardada_sin_giro_ni_orden_sigue_cargando() {
        let json = r#"{"id_instancia":1,"id_mapa":null,"base":{"id":1,"nombre":"Orco","tipo":"Enemigo",
            "cuerpo":"Pequenio","zona_id":1},"coordenadas":"NoPuesto"}"#;
        let i: Instancia = serde_json::from_str(json).unwrap();
        assert_eq!((i.rotacion, i.orden), (0, 0));
    }

    #[test]
    fn la_ficha_que_se_pone_o_se_mueve_queda_encima() {
        let mut e = estado();
        hacer(&mut e, |e| instanciar(e, 1, en(1, 1, 1)));
        hacer(&mut e, |e| instanciar(e, 1, en(1, 1, 1)));
        let capa = |e: &EstadoPartida, id: u16| e.instancias_activas.iter().find(|i| i.id_instancia == id).unwrap().capa;
        assert!(capa(&e, 2) > capa(&e, 1), "la última puesta, encima");

        // La 1 se mueve sobre la 2: ahora es ella la de encima
        hacer(&mut e, |e| mover(e, 1, en(1, 1, 1)));
        assert!(capa(&e, 1) > capa(&e, 2));

        // Quitarla del tablero o girarla no cambia su capa
        let antes = capa(&e, 2);
        hacer(&mut e, |e| rotar(e, 2));
        hacer(&mut e, |e| mover(e, 2, None));
        assert_eq!(capa(&e, 2), antes);

        // Una partida antigua sin `capa` carga con 0
        let json = r#"{"id_instancia":1,"id_mapa":null,"base":{"id":1,"nombre":"Orco","tipo":"Enemigo",
            "cuerpo":"Pequenio","zona_id":1},"coordenadas":"NoPuesto"}"#;
        assert_eq!(serde_json::from_str::<Instancia>(json).unwrap().capa, 0);
    }

    #[test]
    fn un_grupo_se_mueve_conservando_distancias_y_apilado() {
        let mut e = estado(); // mapa 1: 20×10
        hacer(&mut e, |e| instanciar(e, 1, en(1, 2, 2))); // 1
        hacer(&mut e, |e| instanciar(e, 1, en(1, 5, 4))); // 2 (encima de la 1)
        hacer(&mut e, |e| instanciar(e, 1, en(1, 9, 9))); // 3, no se mueve

        let deltas = mover_grupo(&e, &[2, 1], 1, 3, 1).unwrap();
        assert_eq!(deltas.len(), 2);
        for d in &deltas {
            e.aplicar_delta(d);
        }
        let i = |e: &EstadoPartida, id: u16| e.instancias_activas.iter().find(|i| i.id_instancia == id).unwrap().clone();
        assert_eq!(i(&e, 1).coordenadas, Coordenada::Casilla(Casilla { x: 5, y: 3 }));
        assert_eq!(i(&e, 2).coordenadas, Coordenada::Casilla(Casilla { x: 8, y: 5 }));
        // Encima de la 3, y la 2 sigue encima de la 1
        assert!(i(&e, 1).capa > i(&e, 3).capa);
        assert!(i(&e, 2).capa > i(&e, 1).capa);

        // Contra el borde de abajo (y=9): se para cuando la 2 llega, sin juntarse
        for d in mover_grupo(&e, &[1, 2], 1, 0, 50).unwrap() {
            e.aplicar_delta(&d);
        }
        assert_eq!(i(&e, 1).coordenadas, Coordenada::Casilla(Casilla { x: 5, y: 7 }));
        assert_eq!(i(&e, 2).coordenadas, Coordenada::Casilla(Casilla { x: 8, y: 9 }));

        // Errores: otro mapa, una sin colocar, una inexistente o repetida
        assert!(mover_grupo(&e, &[1], 2, 0, 0).is_err());
        hacer(&mut e, |e| instanciar(e, 1, None)); // 4
        assert!(mover_grupo(&e, &[1, 4], 1, 0, 0).is_err());
        assert!(mover_grupo(&e, &[1, 99], 1, 0, 0).is_err());
        assert!(mover_grupo(&e, &[1, 1], 1, 0, 0).is_err());
    }

    #[test]
    fn el_valor_actual_puede_pasar_del_maximo_y_bajar_de_cero() {
        let mut e = estado();
        hacer(&mut e, |e| instanciar(e, 1, None));
        let vida = |e: &EstadoPartida| e.instancias_activas[0].valores_actuales[0].valor;

        // El máximo (30) es el valor por defecto: se puede tener más (vida extra)...
        hacer(&mut e, |e| cambiar_valor_actual(e, 1, ID_VIDA, 5).map(Option::unwrap));
        assert_eq!(vida(&e), 35);
        // ...y menos que cero (vida negativa)
        hacer(&mut e, |e| cambiar_valor_actual(e, 1, ID_VIDA, -100).map(Option::unwrap));
        assert_eq!(vida(&e), -65);
        // Solo hay un tope enorme para no desbordar
        hacer(&mut e, |e| cambiar_valor_actual(e, 1, ID_VIDA, i16::MAX).map(Option::unwrap));
        assert_eq!(vida(&e), LIMITE_VALOR_ACTUAL);
        assert!(cambiar_valor_actual(&e, 1, ID_VIDA, 1).unwrap().is_none(), "ya en el tope");
        // La armadura no tiene máximo
        assert!(cambiar_valor_actual(&e, 1, ID_ARMADURA, 1).is_err());
    }
}
