// src-tauri/src/tablero_general/archivos.rs
use std::fs;
use std::path::{Path, PathBuf};

use crate::tablero_general::modelos::{ArchivoAdjunto, Entidad};

const MAX_BYTES_ARCHIVO: u64 = 150 * 1024 * 1024; // 150 MB

fn subcarpeta_entidad(dir_datos: &Path, entidad_id: u16) -> (PathBuf, String) {
    let rel = if entidad_id == 0 {
        "archivos/temp".to_string()
    } else {
        format!("archivos/entidad_{}", entidad_id)
    };
    let abs = dir_datos.join(&rel);
    (abs, rel)
}

/// Busca el siguiente nombre disponible: Documento_1.txt, Documento_2.txt, etc.
fn siguiente_nombre_secuencial(carpeta: &Path, entidad: &Entidad) -> String {
    let mut n = 1;
    loop {
        let candidato = format!("Documento_{n}.txt");
        let ya_en_entidad = entidad.archivos_adjuntos.iter().flatten().any(|r| {
            r.ends_with(&format!("/{candidato}")) || r == &candidato
        });
        let ya_en_disco = carpeta.join(&candidato).exists();

        if !ya_en_entidad && !ya_en_disco {
            return candidato;
        }
        n += 1;
    }
}

/// Crea un nuevo archivo de texto vacío con nombre limpio (Documento_1.txt, Documento_2.txt...)
pub fn crear_documento_texto(dir_datos: &Path, entidad: Entidad) -> Result<ArchivoAdjunto, String> {
    let (carpeta_abs, carpeta_rel) = subcarpeta_entidad(dir_datos, entidad.id);
    fs::create_dir_all(&carpeta_abs).map_err(|e| format!("No se pudo crear carpeta: {e}"))?;

    let nombre = siguiente_nombre_secuencial(&carpeta_abs, &entidad);
    let destino = carpeta_abs.join(&nombre);

    fs::write(&destino, b"").map_err(|e| format!("Error al crear documento: {e}"))?;

    Ok(ArchivoAdjunto {
        ruta: format!("{carpeta_rel}/{nombre}"),
        entidad_archivo: entidad,
    })
}

/// Guarda o actualiza el texto de un archivo existente
pub fn guardar_texto(
    dir_datos: &Path,
    adjunto: ArchivoAdjunto,
    contenido: &str,
) -> Result<ArchivoAdjunto, String> {
    if adjunto.ruta.trim().is_empty() {
        return Err("Ruta de archivo vacía".into());
    }

    let destino = dir_datos.join(&adjunto.ruta);
    if let Some(padre) = destino.parent() {
        fs::create_dir_all(padre).map_err(|e| format!("No se pudo crear carpeta: {e}"))?;
    }

    fs::write(&destino, contenido.as_bytes()).map_err(|e| format!("Error al guardar texto: {e}"))?;
    Ok(adjunto)
}

/// Renombra el archivo tanto en disco como en el ArchivoAdjunto
pub fn renombrar(
    dir_datos: &Path,
    mut adjunto: ArchivoAdjunto,
    nuevo_nombre: &str,
) -> Result<ArchivoAdjunto, String> {
    let ruta_vieja = adjunto.ruta.clone();
    let path_viejo = dir_datos.join(&ruta_vieja);
    if !path_viejo.exists() {
        return Err("El archivo a renombrar no existe en disco".into());
    }

    let ext_original = path_viejo
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("txt");

    let nombre_saneado = nuevo_nombre
        .trim()
        .replace(|c: char| c == '/' || c == '\\' || c == ':' || c == '*' || c == '?' || c == '"' || c == '<' || c == '>' || c == '|', "_");

    if nombre_saneado.is_empty() {
        return Err("El nombre no puede estar vacío".into());
    }

    // Asegura conservar la extensión si no la puso
    let nombre_final = if nombre_saneado.ends_with(&format!(".{ext_original}")) {
        nombre_saneado
    } else {
        format!("{nombre_saneado}.{ext_original}")
    };

    let carpeta_padre = path_viejo.parent().ok_or("Ruta inválida")?;
    let path_nuevo = carpeta_padre.join(&nombre_final);

    if path_nuevo.exists() && path_nuevo != path_viejo {
        return Err("Ya existe un archivo con ese nombre".into());
    }

    fs::rename(&path_viejo, &path_nuevo).map_err(|e| format!("Error al renombrar: {e}"))?;

    let ruta_nueva = if let Ok(rel) = path_nuevo.strip_prefix(dir_datos) {
        rel.to_string_lossy().replace('\\', "/")
    } else {
        let (_, rel) = subcarpeta_entidad(dir_datos, adjunto.entidad_archivo.id);
        format!("{rel}/{nombre_final}")
    };

    adjunto.ruta = ruta_nueva;
    Ok(adjunto)
}

/// Copia un archivo externo manteniendo su nombre original limpio
pub fn importar(dir_datos: &Path, entidad: Entidad, origen: &Path) -> Result<ArchivoAdjunto, String> {
    let meta = fs::metadata(origen).map_err(|e| format!("No se puede leer el archivo: {e}"))?;
    if !meta.is_file() {
        return Err("La ruta no es un archivo válido".into());
    }
    if meta.len() > MAX_BYTES_ARCHIVO {
        return Err("El archivo supera el tamaño máximo permitido (150 MB)".into());
    }

    let (carpeta_abs, carpeta_rel) = subcarpeta_entidad(dir_datos, entidad.id);
    fs::create_dir_all(&carpeta_abs).map_err(|e| format!("No se pudo crear la carpeta: {e}"))?;

    let nombre_original = origen
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("archivo.bin")
        .replace(|c: char| !c.is_alphanumeric() && c != '.' && c != '-' && c != '_', "_");

    // Si ya existe uno con el mismo nombre en la carpeta de la entidad, le añade _1, _2...
    let mut nombre_final = nombre_original.clone();
    let mut contador = 1;
    while carpeta_abs.join(&nombre_final).exists() {
        let (stem, ext) = match nombre_original.rsplit_once('.') {
            Some((s, e)) => (s, format!(".{e}")),
            None => (nombre_original.as_str(), "".to_string()),
        };
        nombre_final = format!("{stem}_{contador}{ext}");
        contador += 1;
    }

    let destino = carpeta_abs.join(&nombre_final);
    fs::copy(origen, &destino).map_err(|e| format!("Error al copiar archivo: {e}"))?;

    Ok(ArchivoAdjunto {
        ruta: format!("{carpeta_rel}/{nombre_final}"),
        entidad_archivo: entidad,
    })
}

pub fn leer_texto(dir_datos: &Path, ruta_relativa: &str) -> Result<String, String> {
    let destino = dir_datos.join(ruta_relativa);
    if !destino.exists() {
        return Ok(String::new());
    }
    fs::read_to_string(&destino).map_err(|e| format!("No se pudo leer el archivo: {e}"))
}

fn eliminar_si_huerfano(dir_datos: &Path, ruta: &str, banco: &[Entidad], id_excluido: Option<u16>) {
    if ruta.trim().is_empty() { return; }

    let sigue_referenciado = banco.iter().any(|e| {
        if let Some(id) = id_excluido {
            if e.id == id { return false; }
        }
        e.archivos_adjuntos.iter().filter_map(|opt| opt.as_deref()).any(|r| r == ruta)
    });

    if !sigue_referenciado {
        let destino = dir_datos.join(ruta);
        if destino.is_file() {
            let _ = fs::remove_file(destino);
        }
    }
}

pub fn limpiar_desasociados(dir_datos: &Path, anterior: &Entidad, nueva: &Entidad, banco: &[Entidad]) {
    for opt_vieja in &anterior.archivos_adjuntos {
        if let Some(ruta_vieja) = opt_vieja {
            let sigue = nueva.archivos_adjuntos.iter().filter_map(|opt| opt.as_deref()).any(|r| r == ruta_vieja);
            if !sigue {
                eliminar_si_huerfano(dir_datos, ruta_vieja, banco, Some(nueva.id));
            }
        }
    }
}

pub fn limpiar_todos_los_adjuntos(dir_datos: &Path, entidad: &Entidad, banco: &[Entidad]) {
    for opt_ruta in &entidad.archivos_adjuntos {
        if let Some(ruta) = opt_ruta {
            eliminar_si_huerfano(dir_datos, ruta, banco, Some(entidad.id));
        }
    }
    if entidad.id != 0 {
        let carpeta = dir_datos.join(format!("archivos/entidad_{}", entidad.id));
        let _ = fs::remove_dir(&carpeta);
    }
}