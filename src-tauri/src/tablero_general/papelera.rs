// src-tauri/src/tablero_general/papelera.rs
//
// Papelera de reciclaje. Borrar una ficha del banco, un mapa o una zona no destruye nada: lo que se
// quita (con todo lo que arrastra: instancias, mapas de la zona, fichas de la zona...) se guarda
// entero en `EstadoPartida.papelera` como un GRUPO, y Ctrl+Z restaura el último grupo (orden inverso).
// Nunca se deshacen creaciones, solo borrados. Las instancias que se borran solas no van a la papelera.
//
//  * Todo pasa por deltas (`EnviadoAPapelera`, `RestauradoDePapelera`, `PapeleraVaciada`), así que la
//    papelera se guarda en el log y en el snapshot: tras un corte de luz sigue ahí.
//  * Se vacía al cerrar la aplicación de forma normal (y al cambiar de partida). Solo entonces se
//    borran de disco sus imágenes, miniaturas y adjuntos: `recursos::referencias` cuenta también lo
//    que hay en la papelera.
//  * Ids: lo que se crea mientras algo está en la papelera nunca coge su id (`reservados`), así que
//    al restaurar el id siempre está libre y no hay referencias que reescribir. En las fichas es
//    imprescindible: su carpeta de adjuntos lleva el id.
//
// Como en instancias.rs, estas funciones NO cambian el estado: devuelven el delta ya resuelto, y
// `aplicar_envio` / `aplicar_restauracion` (que llama `aplicar_delta`) son deterministas.
use serde::{Deserialize, Serialize};

use crate::tablero_general::estado::{DeltaPartida, EstadoPartida};
use crate::tablero_general::modelos::*;

/// Lo que se quitó al borrar una cosa, con copias completas para poder devolverlo tal cual.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Borrado {
    /// Una ficha del banco y sus instancias.
    Entidad { entidad: Entidad, instancias: Vec<Instancia> },
    /// Un mapa, las instancias no únicas que había en él y cómo estaban las únicas que se quitaron
    /// del tablero (esas no se borran: se desactivan, como siempre).
    Mapa { mapa: Mapa, instancias: Vec<Instancia>, retiradas: Vec<Instancia> },
    /// Una zona con sus mapas, sus fichas no únicas, las instancias que se fueron con ellas y cómo
    /// estaban las instancias únicas que se desactivaron.
    Zona { zona: Zona, mapas: Vec<Mapa>, entidades: Vec<Entidad>, instancias: Vec<Instancia>, retiradas: Vec<Instancia> },
}

/// Lo que se borró de una vez (un Supr con varias fichas es UN grupo). Ctrl+Z restaura un grupo entero.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GrupoPapelera {
    /// Solo para identificar el grupo en `RestauradoDePapelera`.
    pub id: u64,
    pub borrados: Vec<Borrado>,
}

/// Lo que devolvió un Ctrl+Z, para que el front pueda abrir la zona o el mapa recuperados.
#[derive(Serialize, Debug, Default, PartialEq)]
pub struct Restaurado {
    pub zonas: Vec<u16>,
    pub mapas: Vec<u16>,
    pub entidades: Vec<u16>,
}

/// Mayor id de cada tipo que hay en la papelera (0 si ninguno): los ids nuevos empiezan por encima.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Reservados {
    pub zona: u16,
    pub mapa: u16,
    pub entidad: u16,
    pub instancia: u16,
}

pub fn reservados(est: &EstadoPartida) -> Reservados {
    let mut r = Reservados::default();
    let instancias = |lista: &[Instancia], r: &mut Reservados| {
        for i in lista {
            r.instancia = r.instancia.max(i.id_instancia);
        }
    };
    for b in est.papelera.iter().flat_map(|g| &g.borrados) {
        match b {
            Borrado::Entidad { entidad, instancias: inst } => {
                r.entidad = r.entidad.max(entidad.id);
                instancias(inst, &mut r);
            }
            Borrado::Mapa { mapa, instancias: inst, .. } => {
                r.mapa = r.mapa.max(mapa.id);
                instancias(inst, &mut r);
            }
            Borrado::Zona { zona, mapas, entidades, instancias: inst, .. } => {
                r.zona = r.zona.max(zona.id);
                r.mapa = mapas.iter().map(|m| m.id).fold(r.mapa, u16::max);
                r.entidad = entidades.iter().map(|e| e.id).fold(r.entidad, u16::max);
                instancias(inst, &mut r);
            }
        }
    }
    r
}

/// Ids de las fichas que hay en la papelera (para borrar su carpeta de adjuntos al vaciarla).
pub fn ids_entidades(est: &EstadoPartida) -> Vec<u16> {
    est.papelera
        .iter()
        .flat_map(|g| &g.borrados)
        .flat_map(|b| match b {
            Borrado::Entidad { entidad, .. } => vec![entidad.id],
            Borrado::Zona { entidades, .. } => entidades.iter().map(|e| e.id).collect(),
            Borrado::Mapa { .. } => vec![],
        })
        .collect()
}

fn nuevo_grupo(est: &EstadoPartida, borrados: Vec<Borrado>) -> DeltaPartida {
    let id = est.papelera.iter().map(|g| g.id).max().unwrap_or(0) + 1;
    DeltaPartida::EnviadoAPapelera(GrupoPapelera { id, borrados })
}

// ------------------------------------------------------------------
// Borrar (construir el delta)
// ------------------------------------------------------------------

/// Manda a la papelera las fichas del banco `ids` (en ese orden, sin repetir), con sus instancias.
pub fn borrar_entidades(est: &EstadoPartida, ids: &[u16]) -> Result<DeltaPartida, String> {
    let mut vistos = Vec::new();
    let mut borrados = Vec::new();
    for &id in ids {
        if vistos.contains(&id) {
            continue;
        }
        vistos.push(id);
        let entidad = est.banco_entidades.iter().find(|e| e.id == id).ok_or("La ficha ya no existe")?;
        let instancias = est.instancias_activas.iter().filter(|i| i.base.id == id).cloned().collect();
        borrados.push(Borrado::Entidad { entidad: entidad.clone(), instancias });
    }
    if borrados.is_empty() {
        return Err("No hay fichas que borrar".into());
    }
    Ok(nuevo_grupo(est, borrados))
}

/// Separa las instancias afectadas: las no únicas se van; de las únicas se guarda cómo estaban.
fn repartir<'a>(afectadas: impl Iterator<Item = &'a Instancia>) -> (Vec<Instancia>, Vec<Instancia>) {
    afectadas.cloned().partition(|i| !i.base.es_unico)
}

/// Manda a la papelera un mapa con las instancias que había en él.
pub fn borrar_mapa(est: &EstadoPartida, id: u16) -> Result<DeltaPartida, String> {
    let mapa = est.mapas.iter().find(|m| m.id == id).ok_or("Ese mapa no existe")?;
    let (instancias, retiradas) = repartir(est.instancias_activas.iter().filter(|i| i.id_mapa == Some(id)));
    Ok(nuevo_grupo(est, vec![Borrado::Mapa { mapa: mapa.clone(), instancias, retiradas }]))
}

/// Manda a la papelera una zona con sus mapas y sus fichas (las únicas se quedan en el banco y sus
/// instancias solo se desactivan, como siempre).
pub fn borrar_zona(est: &EstadoPartida, id: u16) -> Result<DeltaPartida, String> {
    let zona = est.zonas.iter().find(|z| z.id == id).ok_or("Esa zona no existe")?;
    let mapas: Vec<Mapa> = est.mapas.iter().filter(|m| m.id_zona == id).cloned().collect();
    let ids_mapas: Vec<u16> = mapas.iter().map(|m| m.id).collect();
    let (instancias, retiradas) = repartir(est.instancias_activas.iter().filter(|i| {
        i.id_mapa.is_some_and(|m| ids_mapas.contains(&m)) || i.base.zona_id == id
    }));
    let entidades = est.banco_entidades.iter().filter(|e| !e.es_unico && e.zona_id == id).cloned().collect();
    Ok(nuevo_grupo(est, vec![Borrado::Zona { zona: zona.clone(), mapas, entidades, instancias, retiradas }]))
}

// ------------------------------------------------------------------
// Restaurar (construir el delta)
// ------------------------------------------------------------------

/// Ctrl+Z: el último grupo de la papelera. `None` si está vacía.
pub fn restaurar(est: &EstadoPartida) -> Result<Option<(DeltaPartida, Restaurado)>, String> {
    let Some(grupo) = est.papelera.last() else { return Ok(None) };
    let mut r = Restaurado::default();
    for b in &grupo.borrados {
        match b {
            Borrado::Entidad { entidad, .. } => r.entidades.push(entidad.id),
            Borrado::Mapa { mapa, .. } => {
                // Con la papelera en orden inverso su zona siempre está; si no, no se deja un mapa huérfano
                if !est.zonas.iter().any(|z| z.id == mapa.id_zona) {
                    return Err("No se puede restaurar el mapa: su zona ya no existe".into());
                }
                r.mapas.push(mapa.id);
            }
            Borrado::Zona { zona, mapas, entidades, .. } => {
                r.zonas.push(zona.id);
                r.mapas.extend(mapas.iter().map(|m| m.id));
                r.entidades.extend(entidades.iter().map(|e| e.id));
            }
        }
    }
    Ok(Some((DeltaPartida::RestauradoDePapelera(grupo.id), r)))
}

// ------------------------------------------------------------------
// Aplicar (los llama `aplicar_delta`)
// ------------------------------------------------------------------

/// Quita del estado lo que lleva el grupo y lo guarda en la papelera.
pub fn aplicar_envio(est: &mut EstadoPartida, grupo: &GrupoPapelera) {
    for b in &grupo.borrados {
        match b {
            Borrado::Entidad { entidad, instancias } => {
                est.banco_entidades.retain(|e| e.id != entidad.id);
                quitar_instancias(est, instancias);
                // Y cualquier otra instancia de esa ficha (no debería haberla)
                est.instancias_activas.retain(|i| i.base.id != entidad.id);
            }
            Borrado::Mapa { mapa, instancias, retiradas } => {
                est.mapas.retain(|m| m.id != mapa.id);
                quitar_instancias(est, instancias);
                retirar(est, retiradas, &[mapa.id]);
            }
            Borrado::Zona { zona, mapas, entidades, instancias, retiradas } => {
                let ids_mapas: Vec<u16> = mapas.iter().map(|m| m.id).collect();
                est.zonas.retain(|z| z.id != zona.id);
                est.mapas.retain(|m| !ids_mapas.contains(&m.id));
                est.banco_entidades.retain(|e| !entidades.iter().any(|x| x.id == e.id));
                quitar_instancias(est, instancias);
                retirar(est, retiradas, &ids_mapas);
            }
        }
    }
    est.papelera.push(grupo.clone());
}

fn quitar_instancias(est: &mut EstadoPartida, instancias: &[Instancia]) {
    est.instancias_activas.retain(|i| !instancias.iter().any(|x| x.id_instancia == i.id_instancia));
}

/// Las instancias únicas afectadas no se borran: se desactivan, y salen del tablero si estaban en
/// uno de los mapas que se van.
fn retirar(est: &mut EstadoPartida, retiradas: &[Instancia], mapas: &[u16]) {
    for r in retiradas {
        if let Some(i) = est.instancias_activas.iter_mut().find(|i| i.id_instancia == r.id_instancia) {
            if i.id_mapa.is_some_and(|m| mapas.contains(&m)) {
                i.id_mapa = None;
                i.coordenadas = Coordenada::NoPuesto;
            }
            i.activo = false;
        }
    }
}

/// Saca el grupo `id` de la papelera y lo devuelve al estado.
pub fn aplicar_restauracion(est: &mut EstadoPartida, id: u64) {
    let Some(pos) = est.papelera.iter().position(|g| g.id == id) else { return };
    let grupo = est.papelera.remove(pos);
    for b in grupo.borrados {
        match b {
            Borrado::Entidad { entidad, instancias } => {
                poner_entidad(est, entidad);
                poner_instancias(est, instancias);
            }
            Borrado::Mapa { mapa, instancias, retiradas } => {
                poner_mapa(est, mapa);
                poner_instancias(est, instancias);
                devolver(est, retiradas);
            }
            Borrado::Zona { zona, mapas, entidades, instancias, retiradas } => {
                if !est.zonas.iter().any(|z| z.id == zona.id) {
                    est.zonas.push(zona);
                }
                mapas.into_iter().for_each(|m| poner_mapa(est, m));
                entidades.into_iter().for_each(|e| poner_entidad(est, e));
                poner_instancias(est, instancias);
                devolver(est, retiradas);
            }
        }
    }
    // Pudieron cambiar los parámetros mientras estaba en la papelera
    est.ajustar_a_definiciones();
}

fn poner_entidad(est: &mut EstadoPartida, e: Entidad) {
    if !est.banco_entidades.iter().any(|x| x.id == e.id) {
        est.banco_entidades.push(e);
    }
}

fn poner_mapa(est: &mut EstadoPartida, m: Mapa) {
    if !est.mapas.iter().any(|x| x.id == m.id) {
        est.mapas.push(m);
    }
}

fn poner_instancias(est: &mut EstadoPartida, instancias: Vec<Instancia>) {
    for mut i in instancias {
        if est.instancias_activas.iter().any(|x| x.id_instancia == i.id_instancia) {
            continue;
        }
        // Una ficha única que entretanto volvió a instanciarse se queda con la instancia actual
        if i.base.es_unico && est.instancias_activas.iter().any(|x| x.base.id == i.base.id) {
            continue;
        }
        // Su mapa debería estar (se restaura antes o con ella); si no, vuelve sin colocar
        if i.id_mapa.is_some_and(|m| !est.mapas.iter().any(|x| x.id == m)) {
            i.id_mapa = None;
            i.coordenadas = Coordenada::NoPuesto;
        }
        est.instancias_activas.push(i);
    }
}

/// Las instancias únicas que el borrado sacó del tablero vuelven a su sitio, pero solo si siguen tal
/// como las dejó (sin colocar e inactivas): si el usuario ya las movió, no se tocan.
fn devolver(est: &mut EstadoPartida, retiradas: Vec<Instancia>) {
    for r in retiradas {
        let mapa_existe = r.id_mapa.is_none_or(|m| est.mapas.iter().any(|x| x.id == m));
        if let Some(i) = est.instancias_activas.iter_mut().find(|i| i.id_instancia == r.id_instancia) {
            if !i.activo && mapa_existe && (i.coordenadas == Coordenada::NoPuesto || i.id_mapa == r.id_mapa) {
                i.id_mapa = r.id_mapa;
                i.coordenadas = r.coordenadas;
                i.activo = r.activo;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tablero_general::almacen::{pruebas::dir_temporal, Almacen};
    use crate::tablero_general::estado::cargar;

    fn zona(id: u16) -> Zona {
        Zona { id, nombre: format!("Z{id}"), orden: 0 }
    }
    fn mapa(id: u16, zona: u16) -> Mapa {
        Mapa {
            id, id_zona: zona, nombre: id.to_string(), url_imagen_fondo: None, ancho: 1000, alto: 1000,
            rejilla: 50, color_opacidad: vec![255, 255, 255, 76], grosor: 1, orden: 0, compartido: false,
        }
    }
    fn ficha(id: u16, zona: u16, unica: bool) -> Entidad {
        Entidad {
            id, nombre: format!("F{id}"), tipo: TipoEntidad::Enemigo, parametros: vec![], cuerpo: Cuerpo::Pequenio,
            zona_id: zona, foto_url: None, miniatura_url: None, foto_encuadre: Default::default(),
            archivos_adjuntos: vec![], es_unico: unica, propietarios: vec![],
        }
    }
    fn instancia(id: u16, base: &Entidad, mapa: Option<u16>) -> Instancia {
        Instancia {
            id_instancia: id,
            id_mapa: mapa,
            base: base.clone(),
            valores_actuales: vec![],
            coordenadas: mapa.map_or(Coordenada::NoPuesto, |_| Coordenada::Casilla(Casilla { x: 3, y: 4 })),
            activo: true,
            rotacion: 0,
            espejo: false,
            orden: id,
            capa: 0,
        }
    }

    /// Zona 1 con mapas 1 y 2; zona 2 con mapa 3. Fichas: 1 (zona 1), 2 (zona 1, única), 3 (zona 2).
    /// Instancias: 1 de la ficha 1 en el mapa 1, 2 de la única en el mapa 1, 3 de la ficha 3 en el mapa 3.
    fn estado() -> EstadoPartida {
        let mut e = EstadoPartida { definiciones_parametros: vec![], ..Default::default() };
        e.zonas = vec![zona(1), zona(2)];
        e.mapas = vec![mapa(1, 1), mapa(2, 1), mapa(3, 2)];
        let (f1, f2, f3) = (ficha(1, 1, false), ficha(2, 1, true), ficha(3, 2, false));
        e.instancias_activas = vec![instancia(1, &f1, Some(1)), instancia(2, &f2, Some(1)), instancia(3, &f3, Some(3))];
        e.banco_entidades = vec![f1, f2, f3];
        e
    }

    /// Resumen comparable del estado (los modelos no implementan PartialEq).
    fn foto(e: &EstadoPartida) -> String {
        let mut zonas: Vec<u16> = e.zonas.iter().map(|z| z.id).collect();
        let mut mapas: Vec<(u16, u16)> = e.mapas.iter().map(|m| (m.id, m.id_zona)).collect();
        let mut fichas: Vec<u16> = e.banco_entidades.iter().map(|f| f.id).collect();
        let mut inst: Vec<String> = e
            .instancias_activas
            .iter()
            .map(|i| format!("{}:{}:{:?}:{:?}:{}", i.id_instancia, i.base.id, i.id_mapa, i.coordenadas, i.activo))
            .collect();
        zonas.sort();
        mapas.sort();
        fichas.sort();
        inst.sort();
        format!("{zonas:?} {mapas:?} {fichas:?} {inst:?}")
    }

    fn deshacer(e: &mut EstadoPartida) -> Restaurado {
        let (delta, r) = restaurar(e).unwrap().unwrap();
        e.aplicar_delta(&delta);
        r
    }

    #[test]
    fn borrar_varias_fichas_es_un_grupo_y_ctrl_z_las_devuelve_con_sus_instancias() {
        let mut e = estado();
        let antes = foto(&e);
        let d = borrar_entidades(&e, &[1, 3, 1]).unwrap();
        e.aplicar_delta(&d);
        assert_eq!(e.banco_entidades.iter().map(|f| f.id).collect::<Vec<_>>(), vec![2]);
        assert_eq!(e.instancias_activas.len(), 1);
        assert_eq!(e.papelera.len(), 1);
        assert_eq!(e.papelera[0].borrados.len(), 2, "un solo grupo, sin repetir la 1");

        let r = deshacer(&mut e);
        assert_eq!(r.entidades, vec![1, 3]);
        assert_eq!(foto(&e), antes);
        assert!(e.papelera.is_empty());
        assert!(restaurar(&e).unwrap().is_none(), "papelera vacía: Ctrl+Z no hace nada");
        assert!(borrar_entidades(&e, &[99]).is_err());
    }

    #[test]
    fn un_mapa_vuelve_con_sus_instancias_y_la_unica_recupera_su_sitio() {
        let mut e = estado();
        let antes = foto(&e);
        e.aplicar_delta(&borrar_mapa(&e, 1).unwrap());
        assert!(!e.mapas.iter().any(|m| m.id == 1));
        assert!(!e.instancias_activas.iter().any(|i| i.id_instancia == 1), "la no única se va");
        let unica = e.instancias_activas.iter().find(|i| i.id_instancia == 2).unwrap();
        assert!(!unica.activo && unica.coordenadas == Coordenada::NoPuesto && unica.id_mapa.is_none());

        deshacer(&mut e);
        assert_eq!(foto(&e), antes);
    }

    #[test]
    fn la_unica_que_se_movio_entretanto_no_se_toca() {
        let mut e = estado();
        e.aplicar_delta(&borrar_mapa(&e, 1).unwrap());
        // El usuario vuelve a poner la única en el mapa 2
        let i = e.instancias_activas.iter_mut().find(|i| i.id_instancia == 2).unwrap();
        i.id_mapa = Some(2);
        i.coordenadas = Coordenada::Casilla(Casilla { x: 9, y: 9 });
        i.activo = true;
        deshacer(&mut e);
        let i = e.instancias_activas.iter().find(|i| i.id_instancia == 2).unwrap();
        assert_eq!((i.id_mapa, i.coordenadas), (Some(2), Coordenada::Casilla(Casilla { x: 9, y: 9 })));
    }

    #[test]
    fn una_zona_vuelve_entera_y_los_grupos_se_restauran_en_orden_inverso() {
        let mut e = estado();
        let antes = foto(&e);
        e.aplicar_delta(&borrar_mapa(&e, 2).unwrap());
        e.aplicar_delta(&borrar_zona(&e, 1).unwrap());
        assert!(e.zonas.iter().all(|z| z.id != 1));
        assert!(e.mapas.iter().all(|m| m.id_zona != 1));
        assert_eq!(e.banco_entidades.iter().map(|f| f.id).collect::<Vec<_>>(), vec![2, 3], "la única se queda");

        // Primero la zona (lo último que se borró), luego el mapa
        assert_eq!(deshacer(&mut e), Restaurado { zonas: vec![1], mapas: vec![1], entidades: vec![1] });
        assert_eq!(deshacer(&mut e).mapas, vec![2]);
        assert_eq!(foto(&e), antes);
    }

    #[test]
    fn la_unica_reinstanciada_entretanto_gana_a_la_de_la_papelera() {
        let mut e = estado();
        e.aplicar_delta(&borrar_entidades(&e, &[2]).unwrap());
        // Se vuelve a crear a mano la ficha única con otra instancia (en la práctica no pasa: su id está
        // reservado, pero la restauración no debe duplicar una única)
        let f2 = ficha(2, 1, true);
        e.banco_entidades.push(f2.clone());
        e.instancias_activas.push(instancia(9, &f2, None));
        deshacer(&mut e);
        assert_eq!(e.instancias_activas.iter().filter(|i| i.base.id == 2).count(), 1);
    }

    #[test]
    fn los_ids_de_la_papelera_quedan_reservados() {
        let mut e = estado();
        e.aplicar_delta(&borrar_zona(&e, 2).unwrap());
        assert_eq!(reservados(&e), Reservados { zona: 2, mapa: 3, entidad: 3, instancia: 3 });
        assert_eq!(ids_entidades(&e), vec![3]);
    }

    #[test]
    fn la_papelera_se_reproduce_desde_el_log_y_sobrevive_a_un_cierre_brusco() {
        let dir = dir_temporal("papelera_log");
        {
            let a = Almacen::abrir(&dir).unwrap();
            let mut e = EstadoPartida::default();
            e.registrar(&a, DeltaPartida::ZonaCreada(zona(1))).unwrap();
            e.registrar(&a, DeltaPartida::MapaCreado(zona(1), mapa(1, 1))).unwrap();
            e.registrar(&a, DeltaPartida::EntidadCreada(ficha(1, 1, false))).unwrap();
            e.registrar(&a, DeltaPartida::EntidadCreada(ficha(2, 1, false))).unwrap();
            let d = borrar_entidades(&e, &[1]).unwrap();
            e.registrar(&a, d).unwrap();
            let d = borrar_mapa(&e, 1).unwrap();
            e.registrar(&a, d).unwrap();
            let (d, _) = restaurar(&e).unwrap().unwrap();
            e.registrar(&a, d).unwrap();
            // Sin compactar: como un corte de luz
        }
        let (mut e, _) = cargar(&Almacen::abrir(&dir).unwrap()).unwrap();
        assert_eq!(e.papelera.len(), 1, "queda la ficha 1");
        assert!(e.mapas.iter().any(|m| m.id == 1), "el mapa se restauró");
        assert_eq!(deshacer(&mut e).entidades, vec![1]);

        e.aplicar_delta(&borrar_entidades(&e, &[2]).unwrap());
        e.aplicar_delta(&DeltaPartida::PapeleraVaciada);
        assert!(e.papelera.is_empty());
    }
}
