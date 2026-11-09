// src-tauri/src/tablero_general/cambios_entidad.rs
//
// Cambios de una ficha campo a campo. Al guardar una ficha, el delta `EntidadCambiada` lleva SOLO lo que
// cambió (el nombre, un parámetro, la foto...) en vez de la ficha entera: ocupa poco en el log y, cuando
// haya red, es lo que se envía (filtrado por jugador en `visibilidad::cambios_para`).
//
// `entre` compara dos versiones de la ficha y `aplicar` los vuelve a poner; ninguno toca el estado de la
// partida, y `aplicar` es determinista (lo mismo al reproducir el log que en otro equipo).
use serde::{Deserialize, Serialize};

use crate::tablero_general::modelos::*;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum CambioEntidad {
    Nombre(String),
    Tipo(TipoEntidad),
    /// El valor (o el `publico`) de UN parámetro, el de su `id_definicion`.
    Parametro(Parametro),
    Cuerpo(Cuerpo),
    Zona(u16),
    /// Otra foto: la miniatura propia (recorte de la anterior) deja de valer.
    Foto(Option<String>),
    Encuadre(EncuadreFoto),
    Adjuntos(Vec<Option<String>>),
    Unico(bool),
    Propietarios(Vec<IdJugador>),
}

/// Lo que hay que cambiar en `antes` para que quede como `despues`, campo a campo. Las dos deben estar
/// normalizadas (un parámetro por definición). `miniatura_url` no cuenta: es de Rust
/// (`MiniaturaFichaCambiada`), y una foto nueva ya la anula al aplicarse.
pub fn entre(antes: &Entidad, despues: &Entidad) -> Vec<CambioEntidad> {
    let mut cambios = Vec::new();
    if antes.nombre != despues.nombre {
        cambios.push(CambioEntidad::Nombre(despues.nombre.clone()));
    }
    if antes.tipo != despues.tipo {
        cambios.push(CambioEntidad::Tipo(despues.tipo.clone()));
    }
    for p in &despues.parametros {
        if antes.parametros.iter().find(|a| a.id_definicion == p.id_definicion) != Some(p) {
            cambios.push(CambioEntidad::Parametro(p.clone()));
        }
    }
    if antes.cuerpo != despues.cuerpo {
        cambios.push(CambioEntidad::Cuerpo(despues.cuerpo.clone()));
    }
    if antes.zona_id != despues.zona_id {
        cambios.push(CambioEntidad::Zona(despues.zona_id));
    }
    if antes.foto_url != despues.foto_url {
        cambios.push(CambioEntidad::Foto(despues.foto_url.clone()));
    }
    if antes.foto_encuadre != despues.foto_encuadre {
        cambios.push(CambioEntidad::Encuadre(despues.foto_encuadre));
    }
    if antes.archivos_adjuntos != despues.archivos_adjuntos {
        cambios.push(CambioEntidad::Adjuntos(despues.archivos_adjuntos.clone()));
    }
    if antes.es_unico != despues.es_unico {
        cambios.push(CambioEntidad::Unico(despues.es_unico));
    }
    if antes.propietarios != despues.propietarios {
        cambios.push(CambioEntidad::Propietarios(despues.propietarios.clone()));
    }
    cambios
}

/// Pone los cambios en la ficha, en orden. Un parámetro que la ficha no tiene se ignora (todas siguen
/// las mismas definiciones, así que no debería pasar).
pub fn aplicar(e: &mut Entidad, cambios: &[CambioEntidad]) {
    for c in cambios {
        match c {
            CambioEntidad::Nombre(n) => e.nombre = n.clone(),
            CambioEntidad::Tipo(t) => e.tipo = t.clone(),
            CambioEntidad::Parametro(p) => {
                if let Some(actual) = e.parametros.iter_mut().find(|a| a.id_definicion == p.id_definicion) {
                    *actual = p.clone();
                }
            }
            CambioEntidad::Cuerpo(c) => e.cuerpo = c.clone(),
            CambioEntidad::Zona(z) => e.zona_id = *z,
            CambioEntidad::Foto(f) => {
                e.foto_url = f.clone();
                e.miniatura_url = None;
            }
            CambioEntidad::Encuadre(enc) => e.foto_encuadre = *enc,
            CambioEntidad::Adjuntos(a) => e.archivos_adjuntos = a.clone(),
            CambioEntidad::Unico(u) => e.es_unico = *u,
            CambioEntidad::Propietarios(p) => e.propietarios = p.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ficha() -> Entidad {
        let param = |id, n, publico| Parametro { id_definicion: id, valor: ValorParametro::Numero(n), publico };
        Entidad {
            id: 1,
            nombre: "Orco".into(),
            tipo: TipoEntidad::Enemigo,
            parametros: vec![param(ID_VIDA, 20, false), param(ID_ARMADURA, 12, true)],
            cuerpo: Cuerpo::Pequenio,
            zona_id: 1,
            foto_url: Some("imagenes/a.png".into()),
            miniatura_url: Some("miniaturas/ficha/entidad_1_1.webp".into()),
            foto_encuadre: Default::default(),
            archivos_adjuntos: vec![Some("archivos/entidad_1/notas.txt".into())],
            es_unico: false,
            propietarios: vec![],
        }
    }

    /// Comparación sin `miniatura_url`, que no viaja en los cambios.
    fn sin_miniatura(e: &Entidad) -> String {
        let mut e = e.clone();
        e.miniatura_url = None;
        serde_json::to_string(&e).unwrap()
    }

    #[test]
    fn sin_diferencias_no_hay_cambios() {
        assert!(entre(&ficha(), &ficha()).is_empty());
    }

    #[test]
    fn solo_viaja_lo_que_cambia() {
        let antes = ficha();
        let mut despues = ficha();
        despues.nombre = "Orco jefe".into();
        despues.parametros[1].valor = ValorParametro::Numero(14);
        assert_eq!(
            entre(&antes, &despues),
            vec![
                CambioEntidad::Nombre("Orco jefe".into()),
                CambioEntidad::Parametro(Parametro { id_definicion: ID_ARMADURA, valor: ValorParametro::Numero(14), publico: true }),
            ]
        );
    }

    #[test]
    fn aplicar_los_cambios_deja_la_ficha_igual_que_la_nueva() {
        let antes = ficha();
        let mut despues = ficha();
        despues.nombre = "Troll".into();
        despues.tipo = TipoEntidad::NPC;
        despues.parametros[0] = Parametro { id_definicion: ID_VIDA, valor: ValorParametro::Numero(40), publico: true };
        despues.cuerpo = Cuerpo::Grande(vec![2, 3]);
        despues.zona_id = 4;
        despues.foto_url = Some("imagenes/b.png".into());
        despues.foto_encuadre = EncuadreFoto { x: 10, y: 20, zoom: 150 };
        despues.archivos_adjuntos = vec![];
        despues.es_unico = true;
        despues.propietarios = vec![3];

        let cambios = entre(&antes, &despues);
        assert_eq!(cambios.len(), 10, "un cambio por campo");
        let mut aplicada = antes.clone();
        aplicar(&mut aplicada, &cambios);
        assert_eq!(sin_miniatura(&aplicada), sin_miniatura(&despues));
    }

    #[test]
    fn otra_foto_anula_la_miniatura_propia_y_lo_demas_la_conserva() {
        let mut e = ficha();
        aplicar(&mut e, &[CambioEntidad::Nombre("X".into())]);
        assert!(e.miniatura_url.is_some());
        aplicar(&mut e, &[CambioEntidad::Foto(Some("imagenes/c.png".into()))]);
        assert_eq!(e.miniatura_url, None);
    }

    #[test]
    fn un_parametro_que_la_ficha_no_tiene_se_ignora() {
        let mut e = ficha();
        aplicar(&mut e, &[CambioEntidad::Parametro(Parametro { id_definicion: 99, valor: ValorParametro::Numero(1), publico: true })]);
        assert_eq!(e.parametros, ficha().parametros);
    }
}
