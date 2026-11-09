// src-tauri/src/tablero_general/imagenes.rs
//
// Importa imágenes de mapa al directorio de datos de la app.
//  * Se copian a `<datos>/imagenes/<hash>.<ext>`: el nombre sale del CONTENIDO, así la misma
//    imagen importada dos veces se guarda una sola vez (y dos jugadores tendrán el mismo
//    nombre para la misma imagen cuando haya red).
//  * En la partida solo se guarda la ruta RELATIVA ("imagenes/<hash>.<ext>"): la partida no
//    depende de dónde estuviera el archivo original ni de qué sistema operativo se use.
use crate::tablero_general::archivos;
use image::ImageReader;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

/// Para qué se importa la imagen, que decide qué formatos vale.
///  * `Mapa`: el tablero se dimensiona y se divide en casillas con el tamaño en píxeles de la
///    imagen, así que solo sirven los formatos cuyo tamaño se puede leer (sin SVG ni AVIF).
///  * `Ficha`: la foto va dentro de un contenedor que ya fija el tamaño (la casilla), así que
///    también valen SVG y AVIF.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Uso {
    Mapa,
    Ficha,
}

const EXTENSIONES_MAPA: [&str; 6] = ["png", "jpg", "jpeg", "webp", "gif", "bmp"];
const EXTENSIONES_FICHA: [&str; 8] = ["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif", "svg"];
const MAX_BYTES: u64 = 100 * 1024 * 1024; // 100 MB

impl Uso {
    fn extensiones(self) -> &'static [&'static str] {
        match self {
            Uso::Mapa => &EXTENSIONES_MAPA,
            Uso::Ficha => &EXTENSIONES_FICHA,
        }
    }

    fn nombre(self) -> &'static str {
        match self {
            Uso::Mapa => "un mapa",
            Uso::Ficha => "una ficha",
        }
    }
}

/// Copia `origen` a `<dir_datos>/imagenes/` y devuelve la ruta relativa para guardar en el mapa.
/// Bloqueante (lee y escribe el archivo entero): llamarlo desde un hilo aparte.
pub fn importar(dir_datos: &Path, origen: &Path, uso: Uso) -> Result<String, String> {
    let ext = origen
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .filter(|e| uso.extensiones().contains(&e.as_str()))
        .ok_or_else(|| format!("Formato de imagen no admitido para {}: {}", uso.nombre(), origen.display()))?;

    let meta = fs::metadata(origen).map_err(|e| format!("No se puede leer la imagen: {e}"))?;
    if !meta.is_file() {
        return Err("La ruta no es un archivo".into());
    }
    if meta.len() > MAX_BYTES {
        return Err(format!(
            "La imagen pesa {} MB; el máximo es {} MB",
            meta.len() / (1024 * 1024),
            MAX_BYTES / (1024 * 1024)
        ));
    }

    let bytes = fs::read(origen).map_err(|e| format!("No se puede leer la imagen: {e}"))?;
    guardar_imagen(dir_datos, &bytes, &ext)
}

/// Escribe la imagen en `<dir_datos>/imagenes/<hash>.<ext>` (si no estaba ya) y devuelve su ruta relativa.
fn guardar_imagen(dir_datos: &Path, bytes: &[u8], ext: &str) -> Result<String, String> {
    let hash = format!("{:x}", Sha256::digest(bytes));
    let nombre = format!("{}.{}", &hash[..32], ext);

    let carpeta = dir_datos.join("imagenes");
    fs::create_dir_all(&carpeta).map_err(|e| format!("No se puede crear la carpeta de imágenes: {e}"))?;

    let destino = carpeta.join(&nombre);
    if !destino.exists() {
        // Temporal + rename: nunca queda una imagen a medias con el nombre definitivo.
        let tmp = carpeta.join(format!("{nombre}.tmp"));
        fs::write(&tmp, bytes).map_err(|e| format!("No se pudo copiar la imagen: {e}"))?;
        fs::rename(&tmp, &destino).map_err(|e| format!("No se pudo copiar la imagen: {e}"))?;
    }

    Ok(format!("imagenes/{nombre}"))
}

/// Tamaño máximo de una foto que sube un jugador.
pub const MAX_BYTES_SUBIDA: usize = 8 * 1024 * 1024;
/// Medidas máximas de una foto subida: pocos MB comprimidos pueden ser una imagen gigantesca («bomba») que cuelga a
/// quien la abra (el host al hacer la miniatura o al verla entera, y los navegadores de los demás).
pub const MAX_LADO_SUBIDA: u32 = 8192;
pub const MAX_PIXELES_SUBIDA: u64 = 40_000_000;

/// Medidas que dice la cabecera de una imagen en memoria (sin decodificarla).
pub fn dimensiones_de_bytes(bytes: &[u8]) -> Option<(u32, u32)> {
    ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.into_dimensions().ok()
}

/// La foto de una ficha que SUBE un jugador desde su navegador (no hay ruta del disco del host). Solo imágenes
/// rasterizadas (nada de SVG, que puede llevar scripts) y cuyo contenido sea de verdad una imagen de ese tipo.
pub fn importar_bytes(dir_datos: &Path, bytes: &[u8], extension: &str) -> Result<String, String> {
    let ext = extension.to_ascii_lowercase();
    if !EXTENSIONES_MAPA.contains(&ext.as_str()) {
        return Err("Formato de imagen no admitido (png, jpg, webp, gif o bmp)".into());
    }
    if bytes.is_empty() || bytes.len() > MAX_BYTES_SUBIDA {
        return Err(format!("La imagen no puede pasar de {} MB", MAX_BYTES_SUBIDA / (1024 * 1024)));
    }
    let formato = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.format())
        .ok_or("El archivo no es una imagen")?;
    let esperado: &[&str] = match formato {
        image::ImageFormat::Png => &["png"],
        image::ImageFormat::Jpeg => &["jpg", "jpeg"],
        image::ImageFormat::WebP => &["webp"],
        image::ImageFormat::Gif => &["gif"],
        image::ImageFormat::Bmp => &["bmp"],
        _ => return Err("Formato de imagen no admitido".into()),
    };
    if !esperado.contains(&ext.as_str()) {
        return Err("El archivo no es del tipo que dice su extensión".into());
    }
    let (ancho, alto) = dimensiones_de_bytes(bytes).ok_or("La imagen está dañada")?;
    if ancho == 0 || alto == 0 || ancho > MAX_LADO_SUBIDA || alto > MAX_LADO_SUBIDA || u64::from(ancho) * u64::from(alto) > MAX_PIXELES_SUBIDA {
        return Err(format!("La imagen es demasiado grande (máximo {MAX_LADO_SUBIDA}×{MAX_LADO_SUBIDA} píxeles)"));
    }
    guardar_imagen(dir_datos, bytes, &ext)
}

/// Tamaño en píxeles de una imagen ya importada (`ruta` relativa, "imagenes/..."). Solo lee la
/// cabecera, así que es instantáneo. `None` si no se puede leer
/// (archivo dañado o que no existe). Los lados se recortan a `u16`, que es lo que guarda `Mapa`.
pub fn dimensiones(dir_datos: &Path, ruta: &str) -> Option<(u16, u16)> {
    let archivo = archivos::ruta_segura(dir_datos, ruta, &["imagenes"]).ok()?;
    let (w, h) = ImageReader::open(archivo).ok()?.with_guessed_format().ok()?.into_dimensions().ok()?;
    let lado = |v: u32| v.clamp(1, u32::from(u16::MAX)) as u16;
    Some((lado(w), lado(h)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tablero_general::almacen::pruebas::dir_temporal;

    #[test]
    fn la_misma_imagen_se_guarda_una_vez_y_con_ruta_relativa() {
        let datos = dir_temporal("img_datos");
        let origen = dir_temporal("img_origen");
        fs::create_dir_all(&origen).unwrap();
        fs::write(origen.join("mapa.PNG"), b"contenido-de-prueba").unwrap();
        fs::write(origen.join("copia.png"), b"contenido-de-prueba").unwrap();

        let a = importar(&datos, &origen.join("mapa.PNG"), Uso::Mapa).unwrap();
        let b = importar(&datos, &origen.join("copia.png"), Uso::Mapa).unwrap();

        assert_eq!(a, b, "mismo contenido => mismo nombre");
        assert!(a.starts_with("imagenes/") && a.ends_with(".png"), "{a}");
        assert!(datos.join(&a).is_file());
        assert_eq!(fs::read_dir(datos.join("imagenes")).unwrap().count(), 1);
    }

    #[test]
    fn rechaza_extensiones_y_rutas_no_validas() {
        let datos = dir_temporal("img_rechazo");
        let origen = dir_temporal("img_rechazo_o");
        fs::create_dir_all(&origen).unwrap();
        fs::write(origen.join("script.exe"), b"x").unwrap();

        assert!(importar(&datos, &origen.join("script.exe"), Uso::Mapa).is_err());
        assert!(importar(&datos, &origen.join("no_existe.png"), Uso::Mapa).is_err());
        assert!(importar(&datos, &origen, Uso::Mapa).is_err()); // un directorio
    }

    /// Un BMP que dice medir `ancho`×`alto` (solo la cabecera: no hace falta que traiga los píxeles).
    fn bmp_que_dice(ancho: i32, alto: i32) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"BM");
        b.extend_from_slice(&(54u32 + 16).to_le_bytes());
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(&54u32.to_le_bytes());
        b.extend_from_slice(&40u32.to_le_bytes());
        b.extend_from_slice(&ancho.to_le_bytes());
        b.extend_from_slice(&alto.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&24u16.to_le_bytes());
        b.extend_from_slice(&[0; 24]);
        b.extend_from_slice(&[0; 16]);
        b
    }

    #[test]
    fn una_foto_subida_gigantesca_se_rechaza_aunque_pese_poco() {
        let datos = dir_temporal("img_bomba");
        assert_eq!(dimensiones_de_bytes(&bmp_que_dice(20000, 20000)), Some((20000, 20000)));
        let e = importar_bytes(&datos, &bmp_que_dice(20000, 20000), "bmp").unwrap_err();
        assert!(e.contains("demasiado grande"), "{e}");
        assert!(importar_bytes(&datos, &bmp_que_dice(8000, 8000), "bmp").is_err(), "64 Mpx: pasa del total");

        let mut png = Vec::new();
        image::RgbaImage::new(64, 32).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        assert!(importar_bytes(&datos, &png, "png").is_ok(), "una normal sí");
    }

    #[test]
    fn lee_el_tamano_de_una_imagen_importada() {
        let datos = dir_temporal("img_dims");
        let origen = dir_temporal("img_dims_o");
        fs::create_dir_all(&origen).unwrap();
        image::RgbaImage::new(37, 12).save(origen.join("mapa.png")).unwrap();
        fs::write(origen.join("roto.png"), b"esto no es un png").unwrap();
        fs::write(origen.join("dibujo.svg"), b"<svg/>").unwrap();

        let png = importar(&datos, &origen.join("mapa.png"), Uso::Mapa).unwrap();
        assert_eq!(dimensiones(&datos, &png), Some((37, 12)));

        let roto = importar(&datos, &origen.join("roto.png"), Uso::Mapa).unwrap();
        assert_eq!(dimensiones(&datos, &roto), None);
        assert!(importar(&datos, &origen.join("dibujo.svg"), Uso::Mapa).is_err(), "un mapa no admite SVG");
        // La foto de una ficha sí: va en un contenedor que fija el tamaño, aunque no se pueda medir
        let svg = importar(&datos, &origen.join("dibujo.svg"), Uso::Ficha).unwrap();
        assert_eq!(dimensiones(&datos, &svg), None);
        assert_eq!(dimensiones(&datos, "imagenes/no_existe.png"), None);
        assert_eq!(dimensiones(&datos, "../fuera.png"), None);
    }
}
