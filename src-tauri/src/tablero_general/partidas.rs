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
/// Archivo vacío dentro de `partidas/` mientras una migración está a medias.
const MARCA: &str = ".migrando";
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

/// Borra DEFINITIVAMENTE la carpeta de una partida (sus mapas, fichas, imágenes, archivos y jugadores) y devuelve su
/// nombre. Solo borra una carpeta que `buscar` encuentra dentro de `partidas/` (nunca una ruta que escriba quien llama).
/// Quien llama decide si se puede (p. ej. no la que está abierta).
pub fn borrar(raiz: &Path, nombre: &str) -> Result<String, String> {
    let nombre = buscar(raiz, nombre).ok_or("Esa partida no existe")?;
    fs::remove_dir_all(ruta_partida(raiz, &nombre)).map_err(|e| format!("No se pudo borrar la partida: {e}"))?;
    Ok(nombre)
}

/// Qué haría renombrar `viejo` a `nuevo`, sin tocar el disco: `Err` si no se puede (no existe, el nombre no vale, o ya hay
/// otra partida con ese nombre, sin distinguir mayúsculas), `Ok(None)` si no hay nada que cambiar (el mismo nombre) y
/// `Ok(Some((viejo, nuevo)))` con los nombres definitivos. Un cambio solo de mayúsculas de la misma partida vale.
pub fn plan_renombrado(raiz: &Path, viejo: &str, nuevo: &str) -> Result<Option<(String, String)>, String> {
    let viejo = buscar(raiz, viejo).ok_or("Esa partida no existe")?;
    let nuevo = validar_forma(nuevo)?;
    if nuevo == viejo {
        return Ok(None);
    }
    if buscar(raiz, &nuevo).is_some_and(|otra| otra != viejo) {
        return Err(format!("Ya existe una partida llamada «{nuevo}»"));
    }
    Ok(Some((viejo, nuevo)))
}

/// Renombra la carpeta de una partida y devuelve su nombre definitivo. Si era la «última abierta», se actualiza. Quien llama
/// decide si se puede (la abierta necesita soltar sus archivos antes). Windows tarda a veces en soltar una carpeta
/// (antivirus, indexador): se reintenta unos instantes antes de rendirse.
pub fn renombrar(raiz: &Path, viejo: &str, nuevo: &str) -> Result<String, String> {
    let Some((viejo, nuevo)) = plan_renombrado(raiz, viejo, nuevo)? else {
        return buscar(raiz, viejo).ok_or_else(|| "Esa partida no existe".to_string());
    };
    let (de, a) = (ruta_partida(raiz, &viejo), ruta_partida(raiz, &nuevo));
    let mut ultimo = None;
    for intento in 0..5 {
        match fs::rename(&de, &a) {
            Ok(()) => {
                ultimo = None;
                break;
            }
            Err(e) => {
                ultimo = Some(e);
                if intento < 4 {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        }
    }
    if let Some(e) = ultimo {
        return Err(format!("No se pudo renombrar la partida (¿algún programa tiene sus archivos abiertos?): {e}"));
    }
    if leer_ultima(raiz).as_deref() == Some(viejo.as_str()) {
        guardar_ultima(raiz, &nuevo);
    }
    Ok(nuevo)
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

/// Mueve un archivo o carpeta. En Windows un antivirus, el indexador o el Explorador pueden tener
/// algo abierto un instante y hacer fallar el renombrado: se reintenta unas veces antes de rendirse.
fn mover(desde: &Path, hasta: &Path) -> std::io::Result<()> {
    // Una carpeta vacía en el destino (p. ej. `imagenes/` creada al abrir la partida) no estorba
    if hasta.is_dir() {
        let _ = fs::remove_dir(hasta);
    }
    let mut intento = 0;
    loop {
        match fs::rename(desde, hasta) {
            Ok(()) => return Ok(()),
            Err(e) if intento >= 4 => return Err(e),
            Err(_) => {
                intento += 1;
                std::thread::sleep(std::time::Duration::from_millis(250 * intento));
            }
        }
    }
}

/// Si hay datos del formato antiguo en la raíz y todavía no existe `partidas/`, los MUEVE a
/// `partidas/Partida 1/` (sin copiar: mismo volumen, instantáneo y sin duplicar imágenes).
///
/// Solo se renombran los elementos sueltos, nunca una carpeta que ya contenga a los demás. Una
/// marca (`partidas/.migrando`) dura hasta el final: si algo falla o se corta, el siguiente arranque
/// ve la marca y sigue con lo que quede en la raíz. Devuelve si migró algo.
pub fn migrar_formato_antiguo(raiz: &Path) -> Result<bool, String> {
    let carpeta = carpeta_partidas(raiz);
    let marca = carpeta.join(MARCA);
    let pendientes: Vec<&str> = DATOS_ANTIGUOS.iter().copied().filter(|n| raiz.join(n).exists()).collect();

    let retomando = marca.exists();
    if !retomando && (carpeta.exists() || pendientes.is_empty()) {
        return Ok(false);
    }

    let destino = carpeta.join(PRIMERA);
    fs::create_dir_all(&destino).map_err(|e| format!("No se pudo preparar la migración: {e}"))?;
    fs::write(&marca, "").map_err(|e| format!("No se pudo preparar la migración: {e}"))?;
    for nombre in pendientes {
        mover(&raiz.join(nombre), &destino.join(nombre)).map_err(|e| {
            format!(
                "No se pudo mover «{nombre}» a la carpeta de la partida: {e}. Cierra lo que tenga abierta \
                 la carpeta {} y vuelve a abrir la app: se retomará donde se quedó.",
                raiz.display()
            )
        })?;
    }
    fs::create_dir_all(destino.join("imagenes")).map_err(|e| e.to_string())?;
    let _ = fs::remove_file(&marca);
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
    fn renombrar_mueve_la_carpeta_con_todo_y_no_pisa_a_otra() {
        let r = raiz("renombrar_partidas");
        crear(&r, "A").unwrap();
        crear(&r, "B").unwrap();
        fs::write(ruta_partida(&r, "A").join("partida.json"), "{}").unwrap();
        fs::create_dir_all(ruta_partida(&r, "A").join("archivos/entidad_1")).unwrap();
        fs::write(ruta_partida(&r, "A").join("archivos/entidad_1/n.txt"), "hola").unwrap();
        guardar_ultima(&r, "A");

        assert_eq!(renombrar(&r, "a", "  Campaña  ").unwrap(), "Campaña", "el nombre se limpia y la que busca no distingue mayúsculas");
        assert_eq!(listar(&r), ["B", "Campaña"]);
        assert!(!ruta_partida(&r, "A").exists());
        assert_eq!(fs::read_to_string(ruta_partida(&r, "Campaña").join("archivos/entidad_1/n.txt")).unwrap(), "hola");
        assert!(ruta_partida(&r, "Campaña").join("imagenes").is_dir());
        assert_eq!(leer_ultima(&r).as_deref(), Some("Campaña"), "la «última» sigue a la partida");

        // La que no era la «última» no la toca
        renombrar(&r, "B", "Otra").unwrap();
        assert_eq!(leer_ultima(&r).as_deref(), Some("Campaña"));

        // Nombres que no valen, que existen (también con otras mayúsculas) o que no son de ninguna partida
        for (viejo, nuevo) in [("Otra", ""), ("Otra", "a/b"), ("Otra", "con"), ("Otra", ".."), ("Otra", "campaña"), ("Otra", "CAMPAÑA"), ("Z", "X"), ("../Otra", "X")] {
            assert!(renombrar(&r, viejo, nuevo).is_err(), "{viejo:?} → {nuevo:?}");
        }
        assert_eq!(listar(&r), ["Campaña", "Otra"], "nada cambió");

        // El mismo nombre no hace nada; solo cambiar mayúsculas de la misma partida sí vale
        assert_eq!(renombrar(&r, "Otra", "Otra").unwrap(), "Otra");
        assert_eq!(plan_renombrado(&r, "Otra", "otra").unwrap(), Some(("Otra".into(), "otra".into())));
        assert_eq!(renombrar(&r, "Otra", "otra").unwrap(), "otra");
        assert_eq!(listar(&r), ["Campaña", "otra"]);
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
        assert!(!r.join("partidas").join(".migrando").exists(), "la marca se quita al acabar");
        assert_eq!(listar(&r), [PRIMERA]);

        // Segunda vez: ya existe partidas/, no hace nada aunque aparezcan archivos sueltos
        fs::write(r.join("partida.json"), "{}").unwrap();
        assert!(!migrar_formato_antiguo(&r).unwrap());
        assert!(r.join("partida.json").is_file());
    }

    #[test]
    fn la_migracion_se_retoma_si_se_corto_a_mitad() {
        let r = raiz("p_migrar_corte");
        // Corte tras mover partida.json pero antes de mover imagenes: queda la marca en partidas/
        let p = r.join("partidas").join(PRIMERA);
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("partida.json"), "{}").unwrap();
        fs::write(r.join("partidas").join(".migrando"), "").unwrap();
        fs::create_dir_all(r.join("imagenes")).unwrap();
        fs::write(r.join("imagenes/a.png"), "img").unwrap();
        // Y la app, al abrir esa partida a medias, ya creó su imagenes/ vacía
        fs::create_dir_all(p.join("imagenes")).unwrap();

        assert!(migrar_formato_antiguo(&r).unwrap());
        assert!(p.join("partida.json").is_file() && p.join("imagenes/a.png").is_file());
        assert!(!r.join("imagenes").exists());
        assert!(!r.join("partidas").join(".migrando").exists());
        // Y ya no vuelve a hacer nada
        assert!(!migrar_formato_antiguo(&r).unwrap());
    }

    #[test]
    fn la_migracion_no_renombra_ninguna_carpeta_que_contenga_a_las_demas() {
        // Lo que falló en Windows: renombrar al final la carpeta entera. Ahora `partidas/` nace
        // con ese nombre y solo se mueven los elementos sueltos.
        let r = raiz("p_migrar_sin_tmp");
        fs::write(r.join("partida.json"), "{}").unwrap();
        migrar_formato_antiguo(&r).unwrap();
        let nombres: Vec<_> = fs::read_dir(&r).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(nombres, ["partidas"]);
    }

    #[test]
    fn una_instalacion_nueva_no_migra_nada() {
        let r = raiz("p_nueva");
        assert!(!migrar_formato_antiguo(&r).unwrap());
        assert!(!r.join("partidas").exists());
    }
}
