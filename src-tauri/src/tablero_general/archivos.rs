// src-tauri/src/tablero_general/archivos.rs
//
// Archivos adjuntos de las fichas, en <datos>/archivos/entidad_<id>/.
//
// Normas de este módulo:
//  * Toda ruta que llega del front pasa por `ruta_segura`: nada de rutas absolutas ni de `..`.
//  * No hay "recolector": los archivos solo se borran en el momento en que dejan de usarse
//    (ver `recursos.rs`), nunca por barrido del disco.
//  * Una ficha siempre tiene su id real (se reserva al pulsar "Añadir ficha"), así que no hay
//    carpeta "temp" compartida.
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::tablero_general::modelos::{ArchivoAdjunto, Entidad};

const MAX_BYTES_ARCHIVO: u64 = 150 * 1024 * 1024; // 150 MB
const MAX_LONGITUD_NOMBRE: usize = 120;
pub const RESERVADOS_WINDOWS: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1",
    "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

// ---------------------------------------------------------------------------
// Rutas
// ---------------------------------------------------------------------------

/// Convierte una ruta RELATIVA recibida del front en una ruta absoluta dentro de `dir_datos`.
/// Rechaza rutas absolutas, letras de unidad, `..`, `.` y componentes vacíos, y exige que el
/// primer componente sea uno de `prefijos` (p. ej. "archivos").
pub fn ruta_segura(dir_datos: &Path, ruta: &str, prefijos: &[&str]) -> Result<PathBuf, String> {
    let ruta = ruta.trim();
    if ruta.is_empty() {
        return Err("Ruta vacía".into());
    }
    if ruta.contains(':') || ruta.starts_with('/') || ruta.starts_with('\\') || Path::new(ruta).is_absolute() {
        return Err(format!("Ruta no permitida: {ruta}"));
    }

    let partes: Vec<&str> = ruta.split(['/', '\\']).collect();
    if partes.iter().any(|p| p.is_empty() || *p == "." || *p == "..") {
        return Err(format!("Ruta no permitida: {ruta}"));
    }
    if partes.len() < 2 || !prefijos.contains(&partes[0]) {
        return Err(format!("Ruta fuera de las carpetas permitidas: {ruta}"));
    }

    let mut destino = dir_datos.to_path_buf();
    for p in &partes {
        // Defensa extra: ningún componente puede ser otra cosa que un nombre normal.
        if !matches!(Path::new(p).components().next(), Some(Component::Normal(_))) {
            return Err(format!("Ruta no permitida: {ruta}"));
        }
        destino.push(p);
    }
    Ok(destino)
}

fn carpeta_entidad(dir_datos: &Path, entidad_id: u16) -> Result<(PathBuf, String), String> {
    if entidad_id == 0 {
        return Err("La ficha no tiene id: debe iniciarse con «Añadir ficha» antes de adjuntar archivos".into());
    }
    let rel = format!("archivos/entidad_{entidad_id}");
    Ok((dir_datos.join(&rel), rel))
}

fn nombre_valido(nombre: &str) -> Result<(), String> {
    let stem = nombre.split('.').next().unwrap_or("").to_lowercase();
    if RESERVADOS_WINDOWS.contains(&stem.as_str()) {
        return Err(format!("«{nombre}» es un nombre reservado del sistema"));
    }
    if nombre.chars().count() > MAX_LONGITUD_NOMBRE {
        return Err(format!("El nombre es demasiado largo (máximo {MAX_LONGITUD_NOMBRE} caracteres)"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Documentos de texto
// ---------------------------------------------------------------------------

/// Busca el siguiente nombre disponible: Documento_1.txt, Documento_2.txt, etc.
fn siguiente_nombre_secuencial(carpeta: &Path, entidad: &Entidad) -> String {
    let mut n = 1;
    loop {
        let candidato = format!("Documento_{n}.txt");
        let ya_en_entidad = entidad
            .archivos_adjuntos
            .iter()
            .flatten()
            .any(|r| r.ends_with(&format!("/{candidato}")) || r == &candidato);
        let ya_en_disco = carpeta.join(&candidato).exists();

        if !ya_en_entidad && !ya_en_disco {
            return candidato;
        }
        n += 1;
    }
}

/// Crea un nuevo archivo de texto vacío (Documento_1.txt, Documento_2.txt...)
pub fn crear_documento_texto(dir_datos: &Path, entidad: Entidad) -> Result<ArchivoAdjunto, String> {
    let (carpeta_abs, carpeta_rel) = carpeta_entidad(dir_datos, entidad.id)?;
    fs::create_dir_all(&carpeta_abs).map_err(|e| format!("No se pudo crear carpeta: {e}"))?;

    let nombre = siguiente_nombre_secuencial(&carpeta_abs, &entidad);
    fs::write(carpeta_abs.join(&nombre), b"").map_err(|e| format!("Error al crear documento: {e}"))?;

    Ok(ArchivoAdjunto { ruta: format!("{carpeta_rel}/{nombre}"), entidad_archivo: entidad })
}

/// Actualiza el texto de un archivo EXISTENTE. Si el archivo ya no existe es un error: guardar
/// no debe resucitar en silencio un archivo que se perdió.
pub fn guardar_texto(dir_datos: &Path, adjunto: ArchivoAdjunto, contenido: &str) -> Result<ArchivoAdjunto, String> {
    let destino = ruta_segura(dir_datos, &adjunto.ruta, &["archivos"])?;
    if !destino.is_file() {
        return Err(format!("El archivo «{}» ya no existe", nombre_de(&adjunto.ruta)));
    }
    fs::write(&destino, contenido.as_bytes()).map_err(|e| format!("Error al guardar texto: {e}"))?;
    Ok(adjunto)
}

pub fn leer_texto(dir_datos: &Path, ruta_relativa: &str) -> Result<String, String> {
    let destino = ruta_segura(dir_datos, ruta_relativa, &["archivos"])?;
    if !destino.is_file() {
        return Err(format!("El archivo «{}» no existe", nombre_de(ruta_relativa)));
    }
    fs::read_to_string(&destino).map_err(|e| format!("No se pudo leer el archivo: {e}"))
}

fn nombre_de(ruta: &str) -> &str {
    ruta.rsplit(['/', '\\']).next().unwrap_or(ruta)
}

// ---------------------------------------------------------------------------
// Renombrar
// ---------------------------------------------------------------------------

/// Renombra un adjunto.
///  * `copiar = true`  -> el archivo original se conserva y se crea una copia con el nombre nuevo.
///    Es lo que hay que hacer con un archivo que la partida guardada YA referencia: si luego se
///    cancela la edición no ha pasado nada, y si se guarda, el original se libera solo (recursos.rs).
///  * `copiar = false` -> se mueve (archivo creado en esta misma sesión de edición, que nadie más referencia).
pub fn renombrar(
    dir_datos: &Path,
    mut adjunto: ArchivoAdjunto,
    nuevo_nombre: &str,
    copiar: bool,
) -> Result<ArchivoAdjunto, String> {
    let path_viejo = ruta_segura(dir_datos, &adjunto.ruta, &["archivos"])?;
    if !path_viejo.is_file() {
        return Err("El archivo a renombrar no existe en disco".into());
    }

    let ext_original = path_viejo.extension().and_then(|e| e.to_str()).unwrap_or("txt").to_string();

    let nombre_saneado = nuevo_nombre
        .trim()
        .replace(|c: char| matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'), "_")
        .trim_end_matches(['.', ' '])
        .to_string();
    if nombre_saneado.is_empty() {
        return Err("El nombre no puede estar vacío".into());
    }

    // Conserva la extensión si no la puso
    let nombre_final = if nombre_saneado.to_lowercase().ends_with(&format!(".{}", ext_original.to_lowercase())) {
        nombre_saneado
    } else {
        format!("{nombre_saneado}.{ext_original}")
    };
    nombre_valido(&nombre_final)?;

    let carpeta_padre = path_viejo.parent().ok_or("Ruta inválida")?;
    let path_nuevo = carpeta_padre.join(&nombre_final);

    if path_nuevo == path_viejo {
        return Ok(adjunto);
    }

    // En sistemas que no distinguen mayúsculas (Windows/macOS) "notas.txt" y "Notas.txt" son el MISMO archivo.
    let solo_cambia_mayusculas = path_nuevo.exists()
        && nombre_de(&adjunto.ruta).to_lowercase() == nombre_final.to_lowercase();

    if path_nuevo.exists() && !solo_cambia_mayusculas {
        return Err("Ya existe un archivo con ese nombre".into());
    }

    if solo_cambia_mayusculas {
        if copiar {
            return Err("Solo cambia las mayúsculas: no se puede conservar el original y a la vez renombrarlo. \
                        Cambia algo más del nombre."
                .into());
        }
        // Dos pasos con un nombre intermedio
        let intermedio = carpeta_padre.join(format!(".renombrando_{}", std::process::id()));
        fs::rename(&path_viejo, &intermedio).map_err(|e| format!("Error al renombrar: {e}"))?;
        fs::rename(&intermedio, &path_nuevo).map_err(|e| format!("Error al renombrar: {e}"))?;
    } else if copiar {
        fs::copy(&path_viejo, &path_nuevo).map_err(|e| format!("Error al copiar: {e}"))?;
    } else {
        fs::rename(&path_viejo, &path_nuevo).map_err(|e| format!("Error al renombrar: {e}"))?;
    }

    let rel_padre = adjunto.ruta.replace('\\', "/");
    let rel_padre = rel_padre.rsplit_once('/').map(|(p, _)| p.to_string()).unwrap_or_default();
    adjunto.ruta = format!("{rel_padre}/{nombre_final}");
    Ok(adjunto)
}

// ---------------------------------------------------------------------------
// Importar
// ---------------------------------------------------------------------------

/// Copia un archivo externo a la carpeta de la ficha, con un nombre limpio.
pub fn importar(dir_datos: &Path, entidad: Entidad, origen: &Path) -> Result<ArchivoAdjunto, String> {
    let meta = fs::metadata(origen).map_err(|e| format!("No se puede leer el archivo: {e}"))?;
    if !meta.is_file() {
        return Err("La ruta no es un archivo válido".into());
    }
    if meta.len() > MAX_BYTES_ARCHIVO {
        return Err("El archivo supera el tamaño máximo permitido (150 MB)".into());
    }

    let (carpeta_abs, carpeta_rel) = carpeta_entidad(dir_datos, entidad.id)?;
    fs::create_dir_all(&carpeta_abs).map_err(|e| format!("No se pudo crear la carpeta: {e}"))?;

    let mut nombre_original = origen
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("archivo.bin")
        .replace(|c: char| !c.is_alphanumeric() && c != '.' && c != '-' && c != '_', "_")
        .trim_end_matches('.')
        .to_string();
    if nombre_original.is_empty() || nombre_valido(&nombre_original).is_err() {
        nombre_original = format!("archivo_{nombre_original}");
    }

    // Si ya existe uno con el mismo nombre en la carpeta de la ficha, le añade _1, _2...
    let mut nombre_final = nombre_original.clone();
    let mut contador = 1;
    while carpeta_abs.join(&nombre_final).exists() {
        let (stem, ext) = match nombre_original.rsplit_once('.') {
            Some((s, e)) => (s, format!(".{e}")),
            None => (nombre_original.as_str(), String::new()),
        };
        nombre_final = format!("{stem}_{contador}{ext}");
        contador += 1;
    }

    fs::copy(origen, carpeta_abs.join(&nombre_final)).map_err(|e| format!("Error al copiar archivo: {e}"))?;
    Ok(ArchivoAdjunto { ruta: format!("{carpeta_rel}/{nombre_final}"), entidad_archivo: entidad })
}

// ---------------------------------------------------------------------------
// Borrado explícito (nunca por barrido)
// ---------------------------------------------------------------------------

/// Borra un adjunto y, si su carpeta de ficha queda vacía, la carpeta. Ignora rutas no válidas.
pub fn borrar_archivo(dir_datos: &Path, ruta: &str) -> bool {
    let Ok(destino) = ruta_segura(dir_datos, ruta, &["archivos"]) else { return false };
    if !destino.is_file() {
        return false;
    }
    let borrado = fs::remove_file(&destino).is_ok();
    if let Some(padre) = destino.parent() {
        let _ = fs::remove_dir(padre); // solo funciona si está vacía
    }
    borrado
}

/// Borra la carpeta entera de una ficha. Quien llama debe asegurarse de que esa ficha ya no existe.
pub fn borrar_carpeta_entidad(dir_datos: &Path, entidad_id: u16) {
    if entidad_id == 0 {
        return;
    }
    let _ = fs::remove_dir_all(dir_datos.join(format!("archivos/entidad_{entidad_id}")));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tablero_general::modelos::{Cuerpo, TipoEntidad};
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);
    /// <tmp>/<caso>/datos (dir_datos) y <tmp>/<caso>/fuera (algo ajeno a la app)
    fn entorno() -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("vtt_arch_{}_{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("datos")).unwrap();
        fs::create_dir_all(base.join("fuera")).unwrap();
        (base.join("datos"), base.join("fuera"))
    }

    pub fn entidad(id: u16, adj: &[&str]) -> Entidad {
        Entidad {
            id, nombre: "Orco".into(), tipo: TipoEntidad::Enemigo, parametros: vec![], cuerpo: Cuerpo::Pequenio,
            zona_id: 1, foto_url: None, miniatura_url: None, foto_encuadre: Default::default(), archivos_adjuntos: adj.iter().map(|s| Some(s.to_string())).collect(), es_unico: false,
            propietarios: vec![],
        }
    }

    fn adjunto(ruta: &str, id: u16) -> ArchivoAdjunto {
        ArchivoAdjunto { ruta: ruta.into(), entidad_archivo: entidad(id, &[]) }
    }

    fn escribir(dir: &Path, rel: &str, texto: &str) {
        let p = dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, texto).unwrap();
    }

    #[test]
    fn ruta_segura_acepta_lo_normal_y_rechaza_el_resto() {
        let (d, _) = entorno();
        assert!(ruta_segura(&d, "archivos/entidad_1/Documento_1.txt", &["archivos"]).is_ok());
        assert!(ruta_segura(&d, "imagenes/ab12.png", &["archivos", "imagenes"]).is_ok());
        for mala in [
            "../fuera/x.txt", "archivos/../../fuera/x.txt", "archivos/../x.txt", "/etc/passwd", "\\Windows\\x",
            "C:\\Windows\\x.txt", "C:/x.txt", "archivos//x.txt", "archivos/./x.txt", "otra/x.txt", "archivos",
            "archivos\\..\\..\\x.txt", "", "   ", "archivos/entidad_1/../../../x",
        ] {
            assert!(ruta_segura(&d, mala, &["archivos"]).is_err(), "debía rechazar {mala:?}");
        }
    }

    #[test]
    fn no_se_puede_leer_ni_escribir_ni_renombrar_fuera_de_la_carpeta_de_datos() {
        let (d, fuera) = entorno();
        fs::write(fuera.join("secreto.txt"), "contraseña").unwrap();
        assert!(leer_texto(&d, "../fuera/secreto.txt").is_err());
        assert!(leer_texto(&d, fuera.join("secreto.txt").to_str().unwrap()).is_err());
        assert!(guardar_texto(&d, adjunto("../fuera/pwned.txt", 1), "x").is_err());
        assert!(!fuera.join("pwned.txt").exists());
        assert!(renombrar(&d, adjunto("../fuera/secreto.txt", 1), "otro", false).is_err());
        assert!(fuera.join("secreto.txt").exists());
        assert!(!borrar_archivo(&d, "../fuera/secreto.txt"));
        assert!(fuera.join("secreto.txt").exists());
    }

    #[test]
    fn leer_o_guardar_un_archivo_que_falta_es_un_error_y_no_lo_recrea() {
        let (d, _) = entorno();
        assert!(leer_texto(&d, "archivos/entidad_1/no_existe.txt").is_err());
        assert!(guardar_texto(&d, adjunto("archivos/entidad_1/no_existe.txt", 1), "hola").is_err());
        assert!(!d.join("archivos/entidad_1/no_existe.txt").exists());
    }

    #[test]
    fn crear_guardar_y_leer_un_documento() {
        let (d, _) = entorno();
        let a = crear_documento_texto(&d, entidad(3, &[])).unwrap();
        assert_eq!(a.ruta, "archivos/entidad_3/Documento_1.txt");
        guardar_texto(&d, a.clone(), "mis notas").unwrap();
        assert_eq!(leer_texto(&d, &a.ruta).unwrap(), "mis notas");
        // El siguiente no pisa al primero
        assert_eq!(crear_documento_texto(&d, entidad(3, &[&a.ruta])).unwrap().ruta, "archivos/entidad_3/Documento_2.txt");
    }

    #[test]
    fn una_ficha_sin_id_no_puede_tener_archivos() {
        let (d, fuera) = entorno();
        assert!(crear_documento_texto(&d, entidad(0, &[])).is_err());
        fs::write(fuera.join("a.pdf"), "x").unwrap();
        assert!(importar(&d, entidad(0, &[]), &fuera.join("a.pdf")).is_err());
        assert!(!d.join("archivos").exists());
    }

    #[test]
    fn renombrar_copiando_conserva_el_original_y_moviendo_no() {
        let (d, _) = entorno();
        escribir(&d, "archivos/entidad_5/Documento_1.txt", "notas");

        let copia = renombrar(&d, adjunto("archivos/entidad_5/Documento_1.txt", 5), "Notas", true).unwrap();
        assert_eq!(copia.ruta, "archivos/entidad_5/Notas.txt");
        assert!(d.join("archivos/entidad_5/Documento_1.txt").exists(), "el original debe seguir ahí hasta guardar");
        assert_eq!(fs::read_to_string(d.join("archivos/entidad_5/Notas.txt")).unwrap(), "notas");

        let movido = renombrar(&d, adjunto("archivos/entidad_5/Notas.txt", 5), "Diario.txt", false).unwrap();
        assert_eq!(movido.ruta, "archivos/entidad_5/Diario.txt");
        assert!(!d.join("archivos/entidad_5/Notas.txt").exists());
        assert!(d.join("archivos/entidad_5/Diario.txt").exists());
    }

    #[test]
    fn renombrar_valida_el_nombre() {
        let (d, _) = entorno();
        escribir(&d, "archivos/entidad_5/a.txt", "a");
        escribir(&d, "archivos/entidad_5/b.txt", "b");
        assert!(renombrar(&d, adjunto("archivos/entidad_5/a.txt", 5), "b", false).is_err(), "nombre ya usado");
        assert!(renombrar(&d, adjunto("archivos/entidad_5/a.txt", 5), "CON", false).is_err(), "nombre reservado");
        assert!(renombrar(&d, adjunto("archivos/entidad_5/a.txt", 5), "  ", false).is_err());
        assert!(renombrar(&d, adjunto("archivos/entidad_5/a.txt", 5), &"x".repeat(200), false).is_err());
        let r = renombrar(&d, adjunto("archivos/entidad_5/a.txt", 5), "../../huida. ", false).unwrap();
        assert_eq!(r.ruta, "archivos/entidad_5/.._.._huida.txt", "los separadores se neutralizan y no se sale de la carpeta");
        assert!(d.join("archivos/entidad_5/.._.._huida.txt").exists());
    }

    #[test]
    fn importar_da_nombres_limpios_y_no_pisa() {
        let (d, fuera) = entorno();
        fs::write(fuera.join("mapa del tesoro (1).pdf"), "pdf").unwrap();
        let a = importar(&d, entidad(2, &[]), &fuera.join("mapa del tesoro (1).pdf")).unwrap();
        let b = importar(&d, entidad(2, &[]), &fuera.join("mapa del tesoro (1).pdf")).unwrap();
        assert_eq!(a.ruta, "archivos/entidad_2/mapa_del_tesoro__1_.pdf");
        assert_eq!(b.ruta, "archivos/entidad_2/mapa_del_tesoro__1__1.pdf");
        assert!(importar(&d, entidad(2, &[]), &fuera).is_err(), "un directorio no es un archivo");
    }

    #[test]
    fn borrar_archivo_quita_tambien_la_carpeta_si_queda_vacia() {
        let (d, _) = entorno();
        escribir(&d, "archivos/entidad_8/a.txt", "a");
        escribir(&d, "archivos/entidad_8/b.txt", "b");
        assert!(borrar_archivo(&d, "archivos/entidad_8/a.txt"));
        assert!(d.join("archivos/entidad_8").exists());
        assert!(borrar_archivo(&d, "archivos/entidad_8/b.txt"));
        assert!(!d.join("archivos/entidad_8").exists());
        borrar_carpeta_entidad(&d, 0); // id 0: no hace nada
    }
}
