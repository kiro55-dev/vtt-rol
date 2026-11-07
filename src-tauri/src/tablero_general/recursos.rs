// src-tauri/src/tablero_general/recursos.rs
//
// Qué imágenes y archivos usa la partida, y cuándo dejan de usarse.
//
// En vez de barrer el disco buscando "huérfanos", se compara lo que la partida referenciaba ANTES
// de un cambio con lo que referencia DESPUÉS. Lo que estaba y ya no está, se borra en ese momento.
// Así se cubren todos los casos con una sola regla (borrar un mapa, cambiar su imagen, borrar una
// zona entera, quitar un adjunto, cambiar la foto de una ficha...), y una imagen compartida por
// varios mapas o fichas no se borra hasta que se va el último que la usa.
//
// La comparación se hace DESPUÉS de que el delta esté escrito en el log: si la app se corta entre
// medias, queda un archivo de más, nunca una referencia a un archivo que ya no existe.
use std::collections::HashSet;
use std::path::Path;

use crate::tablero_general::archivos;
use crate::tablero_general::estado::{DeltaPartida, EstadoPartida};
use crate::tablero_general::miniaturas;
use crate::tablero_general::modelos::Entidad;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Referencias {
    /// Rutas relativas "imagenes/..." (fondos de mapa y fotos de ficha)
    pub imagenes: HashSet<String>,
    /// Rutas relativas "archivos/..." (adjuntos de las fichas)
    pub archivos: HashSet<String>,
}

fn es_ruta_de(ruta: &str, carpeta: &str) -> bool {
    ruta.strip_prefix(carpeta).is_some_and(|r| r.starts_with('/'))
}

pub fn referencias(estado: &EstadoPartida) -> Referencias {
    let mut r = Referencias::default();

    for m in &estado.mapas {
        if let Some(u) = m.url_imagen_fondo.as_deref().filter(|u| es_ruta_de(u, "imagenes")) {
            r.imagenes.insert(u.to_string());
        }
    }

    let mut anota = |e: &Entidad| {
        if let Some(f) = e.foto_url.as_deref().filter(|f| es_ruta_de(f, "imagenes")) {
            r.imagenes.insert(f.to_string());
        }
        for a in e.archivos_adjuntos.iter().flatten() {
            if es_ruta_de(a, "archivos") {
                r.archivos.insert(a.clone());
            }
        }
    };
    estado.banco_entidades.iter().for_each(&mut anota);
    estado.instancias_activas.iter().for_each(|i| anota(&i.base));
    r
}

pub fn esta_referenciada(estado: &EstadoPartida, ruta: &str) -> bool {
    let r = referencias(estado);
    r.imagenes.contains(ruta) || r.archivos.contains(ruta)
}

/// ¿Puede este cambio dejar sin uso alguna imagen o archivo?
pub fn afecta_a_recursos(delta: &DeltaPartida) -> bool {
    matches!(
        delta,
        DeltaPartida::ZonaEliminada(_)
            | DeltaPartida::MapaModificado(_)
            | DeltaPartida::MapaBorrado(_)
            | DeltaPartida::EntidadModificada(_)
            | DeltaPartida::EntidadBorrada(_)
    )
}

fn borrar_imagen(dir_datos: &Path, ruta: &str) -> bool {
    let Ok(destino) = archivos::ruta_segura(dir_datos, ruta, &["imagenes"]) else { return false };
    miniaturas::borrar_miniaturas(dir_datos, ruta);
    destino.is_file() && std::fs::remove_file(destino).is_ok()
}

/// Borra lo que estaba en `antes` y ya no está en `despues`. Devuelve las rutas borradas.
pub fn liberar(dir_datos: &Path, antes: &Referencias, despues: &Referencias) -> Vec<String> {
    let mut borradas = Vec::new();
    for ruta in antes.imagenes.difference(&despues.imagenes) {
        if borrar_imagen(dir_datos, ruta) {
            borradas.push(ruta.clone());
        }
    }
    for ruta in antes.archivos.difference(&despues.archivos) {
        if archivos::borrar_archivo(dir_datos, ruta) {
            borradas.push(ruta.clone());
        }
    }
    borradas
}

/// Descarta lo que se creó durante una edición que se canceló (o lo que sobró al guardar:
/// una foto sustituida, un archivo quitado de la lista...). NUNCA toca algo que la partida
/// referencia, así que es seguro llamarlo con cualquier lista.
pub fn descartar(dir_datos: &Path, estado: &EstadoPartida, rutas: &[String]) -> Vec<String> {
    let usadas = referencias(estado);
    let mut borradas = Vec::new();
    for ruta in rutas {
        if usadas.imagenes.contains(ruta) || usadas.archivos.contains(ruta) {
            continue;
        }
        let borrado = if es_ruta_de(ruta, "imagenes") {
            borrar_imagen(dir_datos, ruta)
        } else {
            archivos::borrar_archivo(dir_datos, ruta)
        };
        if borrado {
            borradas.push(ruta.clone());
        }
    }
    borradas
}

/// Siguiente id libre para una ficha nueva, contando también las que se están creando.
pub fn siguiente_id_entidad(banco: &[Entidad], en_creacion: &HashSet<u16>) -> u16 {
    let max_banco = banco.iter().map(|e| e.id).max().unwrap_or(0);
    let max_reservas = en_creacion.iter().copied().max().unwrap_or(0);
    max_banco.max(max_reservas) + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tablero_general::estado::EstadoPartida;
    use crate::tablero_general::modelos::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);
    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("vtt_rec_{}_{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }
    fn crear(d: &Path, rel: &str) {
        let p = d.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "x").unwrap();
    }
    fn existe(d: &Path, rel: &str) -> bool {
        d.join(rel).exists()
    }

    fn mapa(id: u16, zona: u16, imagen: Option<&str>) -> Mapa {
        Mapa {
            id, id_zona: zona, nombre: id.to_string(), url_imagen_fondo: imagen.map(String::from), ancho: 1000, alto: 1000,
            rejilla: 50, color_opacidad: vec![255, 255, 255, 76], grosor: 1, orden: 0,
        }
    }
    fn ficha(id: u16, zona: u16, foto: Option<&str>, adj: &[&str], unica: bool) -> Entidad {
        Entidad {
            id, nombre: format!("F{id}"), tipo: TipoEntidad::Enemigo, parametros: vec![], cuerpo: Cuerpo::Pequenio, zona_id: zona,
            foto_url: foto.map(String::from), archivos_adjuntos: adj.iter().map(|s| Some(s.to_string())).collect(), es_unico: unica,
        }
    }
    fn zona(id: u16) -> Zona {
        Zona { id, nombre: format!("Z{id}"), orden: 0 }
    }

    /// Lo mismo que hace partida.rs al registrar un delta: aplicar y liberar lo que ya no se usa.
    fn aplicar(d: &Path, est: &mut EstadoPartida, delta: DeltaPartida) -> Vec<String> {
        let antes = referencias(est);
        est.aplicar_delta(&delta);
        liberar(d, &antes, &referencias(est))
    }

    /// Crea la imagen y sus miniaturas falsas en disco.
    fn con_miniaturas(d: &Path, rel: &str) {
        crear(d, rel);
        let stem = rel.trim_start_matches("imagenes/").rsplit_once('.').unwrap().0.to_string();
        crear(d, &format!("miniaturas/ficha/{stem}.webp"));
        crear(d, &format!("miniaturas/mapa/{stem}.webp"));
    }

    #[test]
    fn una_imagen_compartida_por_dos_mapas_se_borra_cuando_se_va_el_ultimo() {
        let d = dir();
        con_miniaturas(&d, "imagenes/aa.png");
        let mut est = EstadoPartida::default();
        est.mapas = vec![mapa(1, 1, Some("imagenes/aa.png")), mapa(2, 1, Some("imagenes/aa.png"))];

        assert!(aplicar(&d, &mut est, DeltaPartida::MapaBorrado(mapa(1, 1, None))).is_empty());
        assert!(existe(&d, "imagenes/aa.png"), "otro mapa la sigue usando");

        let borradas = aplicar(&d, &mut est, DeltaPartida::MapaBorrado(mapa(2, 1, None)));
        assert_eq!(borradas, vec!["imagenes/aa.png"]);
        assert!(!existe(&d, "imagenes/aa.png"));
        assert!(!existe(&d, "miniaturas/mapa/aa.webp") && !existe(&d, "miniaturas/ficha/aa.webp"), "y sus miniaturas");
    }

    #[test]
    fn cambiar_la_imagen_de_un_mapa_borra_la_anterior_salvo_que_la_use_otra_cosa() {
        let d = dir();
        con_miniaturas(&d, "imagenes/vieja.png");
        con_miniaturas(&d, "imagenes/nueva.png");
        con_miniaturas(&d, "imagenes/compartida.png");
        let mut est = EstadoPartida::default();
        est.mapas = vec![mapa(1, 1, Some("imagenes/vieja.png")), mapa(2, 1, Some("imagenes/compartida.png"))];
        est.banco_entidades = vec![ficha(1, 1, Some("imagenes/compartida.png"), &[], false)];

        aplicar(&d, &mut est, DeltaPartida::MapaModificado(mapa(1, 1, Some("imagenes/nueva.png"))));
        assert!(!existe(&d, "imagenes/vieja.png") && existe(&d, "imagenes/nueva.png"));

        // La foto de una ficha cuenta como uso: el mapa 2 suelta la imagen y se conserva
        aplicar(&d, &mut est, DeltaPartida::MapaModificado(mapa(2, 1, None)));
        assert!(existe(&d, "imagenes/compartida.png"));
    }

    #[test]
    fn quitar_un_adjunto_borra_el_archivo_y_la_carpeta_vacia() {
        let d = dir();
        crear(&d, "archivos/entidad_1/a.txt");
        crear(&d, "archivos/entidad_1/b.txt");
        let mut est = EstadoPartida::default();
        est.banco_entidades = vec![ficha(1, 1, None, &["archivos/entidad_1/a.txt", "archivos/entidad_1/b.txt"], false)];

        aplicar(&d, &mut est, DeltaPartida::EntidadModificada(ficha(1, 1, None, &["archivos/entidad_1/b.txt"], false)));
        assert!(!existe(&d, "archivos/entidad_1/a.txt") && existe(&d, "archivos/entidad_1/b.txt"));
        aplicar(&d, &mut est, DeltaPartida::EntidadModificada(ficha(1, 1, None, &[], false)));
        assert!(!existe(&d, "archivos/entidad_1"));
    }

    #[test]
    fn renombrar_copiando_y_guardar_libera_el_original_y_cancelar_no_toca_nada() {
        let d = dir();
        fs::create_dir_all(d.join("archivos/entidad_5")).unwrap();
        fs::write(d.join("archivos/entidad_5/Documento_1.txt"), "notas").unwrap();
        let persistida = ficha(5, 1, None, &["archivos/entidad_5/Documento_1.txt"], false);
        let mut est = EstadoPartida::default();
        est.banco_entidades = vec![persistida.clone()];

        // El usuario renombra: se COPIA, y el original sigue referenciado
        let adj = ArchivoAdjunto { ruta: "archivos/entidad_5/Documento_1.txt".into(), entidad_archivo: persistida.clone() };
        let copia = crate::tablero_general::archivos::renombrar(&d, adj, "Notas", true).unwrap();

        // --- Cancelar: se descarta la copia; la ficha guardada sigue intacta ---
        let borradas = descartar(&d, &est, &[copia.ruta.clone()]);
        assert_eq!(borradas, vec![copia.ruta.clone()]);
        assert_eq!(fs::read_to_string(d.join("archivos/entidad_5/Documento_1.txt")).unwrap(), "notas");

        // --- Guardar: la ficha pasa a apuntar a la copia y el original se libera solo ---
        let copia = crate::tablero_general::archivos::renombrar(
            &d,
            ArchivoAdjunto { ruta: "archivos/entidad_5/Documento_1.txt".into(), entidad_archivo: persistida },
            "Notas",
            true,
        )
        .unwrap();
        aplicar(&d, &mut est, DeltaPartida::EntidadModificada(ficha(5, 1, None, &[&copia.ruta], false)));
        assert!(!existe(&d, "archivos/entidad_5/Documento_1.txt"));
        assert_eq!(fs::read_to_string(d.join("archivos/entidad_5/Notas.txt")).unwrap(), "notas");
    }

    #[test]
    fn borrar_una_zona_libera_lo_de_sus_mapas_y_fichas_pero_no_las_fichas_unicas() {
        let d = dir();
        for r in ["imagenes/mapa_z1.png", "imagenes/foto_orco.png", "imagenes/foto_unica.png", "imagenes/mapa_z2.png"] {
            con_miniaturas(&d, r);
        }
        for r in ["archivos/entidad_1/a.txt", "archivos/entidad_2/b.txt"] {
            crear(&d, r);
        }
        let mut est = EstadoPartida::default();
        est.zonas = vec![zona(1), zona(2)];
        est.mapas = vec![mapa(1, 1, Some("imagenes/mapa_z1.png")), mapa(2, 2, Some("imagenes/mapa_z2.png"))];
        est.banco_entidades = vec![
            ficha(1, 1, Some("imagenes/foto_orco.png"), &["archivos/entidad_1/a.txt"], false),
            ficha(2, 1, Some("imagenes/foto_unica.png"), &["archivos/entidad_2/b.txt"], true), // única: se conserva
        ];

        aplicar(&d, &mut est, DeltaPartida::ZonaEliminada(zona(1)));

        assert!(!existe(&d, "imagenes/mapa_z1.png") && !existe(&d, "imagenes/foto_orco.png") && !existe(&d, "archivos/entidad_1/a.txt"));
        assert!(existe(&d, "imagenes/foto_unica.png") && existe(&d, "archivos/entidad_2/b.txt"), "la ficha única sobrevive con sus cosas");
        assert!(existe(&d, "imagenes/mapa_z2.png"), "la otra zona no se toca");
    }

    #[test]
    fn borrar_una_ficha_libera_sus_adjuntos_y_su_foto_salvo_si_la_usa_un_mapa() {
        let d = dir();
        con_miniaturas(&d, "imagenes/foto.png");
        con_miniaturas(&d, "imagenes/fondo.png");
        crear(&d, "archivos/entidad_1/a.txt");
        crear(&d, "archivos/entidad_2/b.txt");
        let mut est = EstadoPartida::default();
        est.mapas = vec![mapa(1, 1, Some("imagenes/fondo.png"))];
        est.banco_entidades = vec![
            ficha(1, 1, Some("imagenes/foto.png"), &["archivos/entidad_1/a.txt"], false),
            ficha(2, 1, Some("imagenes/fondo.png"), &["archivos/entidad_2/b.txt"], false),
        ];
        aplicar(&d, &mut est, DeltaPartida::EntidadBorrada(ficha(1, 1, None, &[], false)));
        assert!(!existe(&d, "imagenes/foto.png") && !existe(&d, "archivos/entidad_1/a.txt"));

        aplicar(&d, &mut est, DeltaPartida::EntidadBorrada(ficha(2, 1, None, &[], false)));
        assert!(existe(&d, "imagenes/fondo.png"), "el mapa 1 aún usa esa imagen");
        assert!(!existe(&d, "archivos/entidad_2/b.txt"));
    }

    #[test]
    fn descartar_nunca_toca_lo_que_la_partida_referencia_ni_sale_de_las_carpetas() {
        let d = dir();
        crear(&d, "archivos/entidad_1/usado.txt");
        crear(&d, "archivos/entidad_1/sobrante.txt");
        con_miniaturas(&d, "imagenes/usada.png");
        con_miniaturas(&d, "imagenes/sobrante.png");
        fs::write(d.join("ajeno.txt"), "ajeno").unwrap();
        let mut est = EstadoPartida::default();
        est.mapas = vec![mapa(1, 1, Some("imagenes/usada.png"))];
        est.banco_entidades = vec![ficha(1, 1, None, &["archivos/entidad_1/usado.txt"], false)];

        let lista: Vec<String> = [
            "archivos/entidad_1/usado.txt", "archivos/entidad_1/sobrante.txt", "imagenes/usada.png", "imagenes/sobrante.png",
            "../ajeno.txt", "archivos/../ajeno.txt", "ajeno.txt",
        ]
        .map(String::from)
        .to_vec();
        let mut borradas = descartar(&d, &est, &lista);
        borradas.sort();

        assert_eq!(borradas, vec!["archivos/entidad_1/sobrante.txt", "imagenes/sobrante.png"]);
        assert!(existe(&d, "archivos/entidad_1/usado.txt") && existe(&d, "imagenes/usada.png") && existe(&d, "ajeno.txt"));
        assert!(!existe(&d, "miniaturas/mapa/sobrante.webp"));
    }

    #[test]
    fn solo_cuentan_las_rutas_de_la_app_no_las_urls_antiguas() {
        let mut est = EstadoPartida::default();
        est.mapas = vec![mapa(1, 1, Some("http://asset.localhost/C%3A/x.png")), mapa(2, 1, Some("data:image/png;base64,AAA")), mapa(3, 1, None)];
        est.banco_entidades = vec![ficha(1, 1, Some("../../fuera.png"), &["otra/cosa.txt"], false)];
        assert_eq!(referencias(&est), Referencias::default());
    }

    #[test]
    fn el_id_de_una_ficha_nueva_tiene_en_cuenta_las_que_se_estan_creando() {
        let banco = vec![ficha(1, 1, None, &[], false), ficha(4, 1, None, &[], false)];
        let mut reservas = HashSet::new();
        assert_eq!(siguiente_id_entidad(&banco, &reservas), 5);
        reservas.insert(5);
        reservas.insert(6);
        assert_eq!(siguiente_id_entidad(&banco, &reservas), 7);
        assert_eq!(siguiente_id_entidad(&[], &HashSet::new()), 1);
    }

    #[test]
    fn solo_los_cambios_que_pueden_liberar_algo_se_vigilan() {
        assert!(afecta_a_recursos(&DeltaPartida::MapaBorrado(mapa(1, 1, None))));
        assert!(afecta_a_recursos(&DeltaPartida::EntidadModificada(ficha(1, 1, None, &[], false))));
        assert!(!afecta_a_recursos(&DeltaPartida::ValorActualModificado { id_instancia: 1, id_definicion: 1, delta: 1 }));
        assert!(!afecta_a_recursos(&DeltaPartida::ZonaCreada(zona(1))));
    }
}
