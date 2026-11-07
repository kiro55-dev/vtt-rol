// src-tauri/src/tablero_general/partidas.rs
//
// Las partidas: cada una es una carpeta dentro de `<datos de la app>/partidas/`. Este módulo solo
// sabe de carpetas (listar, crear, validar nombres, recordar la última abierta, migrar el formato
// antiguo); abrir una partida de verdad lo hace `partida.rs`, que tiene el estado.
//
//   <raiz>/config.json                 { "ultima_partida": "Mi campaña" }
//   <raiz>/partidas/<nombre>/          partida.json, deltas.jsonl, imagenes/, archivos/, miniaturas/...
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::tablero_general::archivos::RESERVADOS_WINDOWS;

const CARPETA: &str = "partidas";
const CARPETA_TMP: &str = "partidas.migrando";
const CONFIG: &str = "config.json";
const MAX_NOMBRE: usize = 40;
const PROHIBIDOS: &str = "\\/:*?\"<>|";
/// Nombre de la partida que se crea sola la primera vez, y de la que recibe los datos del formato antiguo.
pub const PRIMERA: &str = "Partida 1";

/// Lo que el formato antiguo dejaba suelto en la raíz y ahora vive dentro de cada partida.
const DATOS_ANTIGUOS: [&str; 6] =
    ["partida.json", "deltas.jsonl", "historial_personal.txt", "imagenes", "archivos", "miniaturas"];

#[derive(Serialize, Deserialize, Default)]
struct Config {
    #[serde(default)]
    ultima_partida: Option<String>,
}

pub fn carpeta_partidas(raiz: &Path) -> PathBuf {
    raiz.join(CARPETA)
}

/// Ruta de una partida. El nombre debe haber pasado por `validar_forma` (no se comprueba aquí).
pub fn ruta_partida(raiz: &Path, nombre: &str) -> PathBuf {
    carpeta_partidas(raiz).join(nombre)
}

/// Comprueba que el nombre vale como nombre de carpeta (sin mirar si ya existe) y lo devuelve
/// sin espacios sobrantes. Impide también rutas: ni separadores ni `..`.
pub fn validar_forma(nombre: &str) -> Result<String, String> {
    let nombre = nombre.trim();
    if nombre.is_empty() {
        return Err("La partida necesita un nombre".into());
    }
    if nombre.chars().count() > MAX_NOMBRE {
        return Err(format!("El nombre no puede pasar de {MAX_NOMBRE} caracteres"));
    }
    if let Some(c) = nombre.chars().find(|c| c.is_control() || PROHIBIDOS.contains(*c)) {
        return Err(format!("El nombre no puede llevar el carácter «{c}»"));
    }
    if nombre.ends_with('.') {
        return Err("El nombre no puede acabar en punto".into());
    }
    let tronco = nombre.split('.').next().unwrap_or("").to_lowercase();
    if RESERVADOS_WINDOWS.contains(&tronco.as_str()) {
        return Err(format!("«{nombre}» es un nombre reservado del sistema"));
    }
    Ok(nombre.to_string())
}

/// Subcarpetas de `partidas/`, por orden alfabético.
pub fn listar(raiz: &Path) -> Vec<String> {
    let mut nombres: Vec<String> = fs::read_dir(carpeta_partidas(raiz))
        .map(|it| {
            it.flatten()
                .filter(|e| e.path().is_dir())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    nombres.sort_by_key(|n| n.to_lowercase());
    nombres
}

/// El nombre tal como está en disco, si existe una partida con ese nombre (sin mirar mayúsculas).
pub fn buscar(raiz: &Path, nombre: &str) -> Option<String> {
    let buscado = nombre.trim().to_lowercase();
    listar(raiz).into_iter().find(|n| n.to_lowercase() == buscado)
}

/// Crea la carpeta de una partida nueva y devuelve su nombre definitivo.
pub fn crear(raiz: &Path, nombre: &str) -> Result<String, String> {
    let nombre = validar_forma(nombre)?;
    if buscar(raiz, &nombre).is_some() {
        return Err(format!("Ya existe una partida llamada «{nombre}»"));
    }
    fs::create_dir_all(ruta_partida(raiz, &nombre).join("imagenes"))
        .map_err(|e| format!("No se pudo crear la partida: {e}"))?;
    Ok(nombre)
}

// ------------------------------------------------------------------
// Última partida abierta
// ------------------------------------------------------------------

pub fn leer_ultima(raiz: &Path) -> Option<String> {
    let texto = fs::read_to_string(raiz.join(CONFIG)).ok()?;
    serde_json::from_str::<Config>(&texto).ok()?.ultima_partida
}

pub fn guardar_ultima(raiz: &Path, nombre: &str) {
    let config = Config { ultima_partida: Some(nombre.to_string()) };
    let Ok(json) = serde_json::to_string_pretty(&config) else { return };
    if let Err(e) = fs::write(raiz.join(CONFIG), json) {
        eprintln!("[partidas] no se pudo recordar la última partida: {e}");
    }
}

/// Qué partida abrir al arrancar: la última si sigue existiendo, si no la primera de la lista, y si
/// no hay ninguna se crea «Partida 1». El `bool` dice si se acaba de crear desde cero.
pub fn elegir_inicial(raiz: &Path) -> Result<(String, bool), String> {
    if let Some(n) = leer_ultima(raiz).and_then(|u| buscar(raiz, &u)) {
        return Ok((n, false));
    }
    if let Some(n) = listar(raiz).into_iter().next() {
        return Ok((n, false));
    }
    Ok((crear(raiz, PRIMERA)?, true))
}

// ------------------------------------------------------------------
// Migración del formato antiguo (una sola partida suelta en la raíz)
// ------------------------------------------------------------------

/// Si hay datos del formato antiguo en la raíz y todavía no existe `partidas/`, los MUEVE a
/// `partidas/Partida 1/` (sin copiar: mismo volumen, instantáneo y sin duplicar imágenes).
/// Se hace dentro de `partidas.migrando/` y solo al final se renombra a `partidas/`, así un corte a
/// mitad se retoma en el siguiente arranque sin perder nada. Devuelve si migró algo.
pub fn migrar_formato_antiguo(raiz: &Path) -> Result<bool, String> {
    if carpeta_partidas(raiz).exists() {
        return Ok(false);
    }
    let tmp = raiz.join(CARPETA_TMP);
    let pendientes: Vec<&str> = DATOS_ANTIGUOS.iter().copied().filter(|n| raiz.join(n).exists()).collect();
    if pendientes.is_empty() && !tmp.exists() {
        return Ok(false);
    }

    let destino = tmp.join(PRIMERA);
    fs::create_dir_all(&destino).map_err(|e| format!("No se pudo preparar la migración: {e}"))?;
    for nombre in pendientes {
        fs::rename(raiz.join(nombre), destino.join(nombre))
            .map_err(|e| format!("No se pudo mover «{nombre}» a la carpeta de la partida: {e}"))?;
    }
    fs::create_dir_all(destino.join("imagenes")).map_err(|e| e.to_string())?;
    fs::rename(&tmp, carpeta_partidas(raiz)).map_err(|e| format!("No se pudo terminar la migración: {e}"))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tablero_general::almacen::pruebas::dir_temporal;

    fn raiz(nombre: &str) -> PathBuf {
        let d = dir_temporal(nombre);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn validar_forma_acepta_nombres_normales_y_rechaza_el_resto() {
        assert_eq!(validar_forma("  Mi campaña 2  ").unwrap(), "Mi campaña 2");
        for malo in ["", "   ", "a/b", "a\\b", "..", "a.", "con", "NUL.txt", "a:b", "a*b", "a\tb", &"x".repeat(41)] {
            assert!(validar_forma(malo).is_err(), "debía rechazar {malo:?}");
        }
        assert!(validar_forma(&"x".repeat(40)).is_ok());
    }

    #[test]
    fn crear_y_listar_sin_distinguir_mayusculas_al_repetir() {
        let r = raiz("p_crear");
        assert_eq!(crear(&r, " Zeta ").unwrap(), "Zeta");
        crear(&r, "alfa").unwrap();
        assert_eq!(listar(&r), ["alfa", "Zeta"]);
        assert!(r.join("partidas/alfa/imagenes").is_dir());
        assert!(crear(&r, "ALFA").unwrap_err().contains("Ya existe"));
        assert_eq!(buscar(&r, "ZETA").as_deref(), Some("Zeta"));
        assert_eq!(buscar(&r, "nada"), None);
    }

    #[test]
    fn elegir_inicial_usa_la_ultima_luego_la_primera_y_si_no_crea_la_primera() {
        let r = raiz("p_inicial");
        assert_eq!(elegir_inicial(&r).unwrap(), (PRIMERA.to_string(), true));
        crear(&r, "Beta").unwrap();
        guardar_ultima(&r, "Beta");
        assert_eq!(elegir_inicial(&r).unwrap(), ("Beta".to_string(), false));
        // La última ya no existe: la primera de la lista
        guardar_ultima(&r, "Borrada");
        assert_eq!(elegir_inicial(&r).unwrap(), ("Beta".to_string(), false));
        // Config ilegible: se ignora
        fs::write(r.join("config.json"), "{ roto").unwrap();
        assert_eq!(leer_ultima(&r), None);
    }

    #[test]
    fn la_migracion_mueve_los_datos_antiguos_a_partida_1_y_no_se_repite() {
        let r = raiz("p_migrar");
        fs::write(r.join("partida.json"), "{}").unwrap();
        fs::write(r.join("deltas.jsonl"), "").unwrap();
        fs::create_dir_all(r.join("imagenes")).unwrap();
        fs::write(r.join("imagenes/a.png"), "img").unwrap();
        fs::create_dir_all(r.join("archivos/entidad_1")).unwrap();
        fs::write(r.join("copia.zip"), "zip").unwrap();

        assert!(migrar_formato_antiguo(&r).unwrap());
        let p = r.join("partidas").join(PRIMERA);
        assert_eq!(fs::read_to_string(p.join("imagenes/a.png")).unwrap(), "img");
        assert!(p.join("partida.json").is_file() && p.join("archivos/entidad_1").is_dir());
        assert!(!r.join("partida.json").exists() && !r.join("imagenes").exists(), "se mueven, no se copian");
        assert!(r.join("copia.zip").is_file(), "lo que no es de la partida no se toca");
        assert!(!r.join("partidas.migrando").exists());
        assert_eq!(listar(&r), [PRIMERA]);

        // Segunda vez: ya existe partidas/, no hace nada aunque aparezcan archivos sueltos
        fs::write(r.join("partida.json"), "{}").unwrap();
        assert!(!migrar_formato_antiguo(&r).unwrap());
        assert!(r.join("partida.json").is_file());
    }

    #[test]
    fn la_migracion_se_retoma_si_se_corto_a_mitad() {
        let r = raiz("p_migrar_corte");
        // Corte tras mover partida.json pero antes de mover imagenes y de renombrar la carpeta
        fs::create_dir_all(r.join("partidas.migrando").join(PRIMERA)).unwrap();
        fs::write(r.join("partidas.migrando").join(PRIMERA).join("partida.json"), "{}").unwrap();
        fs::create_dir_all(r.join("imagenes")).unwrap();
        fs::write(r.join("imagenes/a.png"), "img").unwrap();

        assert!(migrar_formato_antiguo(&r).unwrap());
        let p = r.join("partidas").join(PRIMERA);
        assert!(p.join("partida.json").is_file() && p.join("imagenes/a.png").is_file());
    }

    #[test]
    fn una_instalacion_nueva_no_migra_nada() {
        let r = raiz("p_nueva");
        assert!(!migrar_formato_antiguo(&r).unwrap());
        assert!(!r.join("partidas").exists());
    }
}
