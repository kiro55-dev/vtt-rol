// src-tauri/src/tablero_general/miniaturas.rs
//
// Miniaturas de imágenes (fondos de mapa y fotos de ficha), como archivos WebP lossy al 90 %
// dentro de <datos>/miniaturas/<tipo>/<hash>.webp.
//
//  * Se generan UNA vez, bajo demanda (la primera vez que una imagen se va a mostrar), con
//    recorte centrado ("cover") para que no se deformen.
//  * El front solo carga el archivo resultante: no decodifica la foto original ni guarda
//    miniaturas en memoria de JavaScript.
//  * El nombre sale del de la imagen original (que ya es el hash de su contenido), así que
//    una imagen nueva siempre tiene miniatura nueva y no hay que invalidar nada.
//  * Formatos que no se pueden reducir aquí (SVG, AVIF) o imágenes corruptas devuelven
//    `None`: el front usa entonces la imagen original.
use std::fs;
use std::path::Path;

use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageReader};

use crate::tablero_general::archivos::ruta_segura;

/// Calidad del WebP (0-100).
pub const CALIDAD_WEBP: f32 = 90.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tipo {
    Ficha,
    Mapa,
}

impl Tipo {
    pub const TODOS: [Tipo; 2] = [Tipo::Ficha, Tipo::Mapa];

    pub fn desde_texto(t: &str) -> Option<Tipo> {
        match t {
            "ficha" => Some(Tipo::Ficha),
            "mapa" => Some(Tipo::Mapa),
            _ => None,
        }
    }

    fn carpeta(self) -> &'static str {
        match self {
            Tipo::Ficha => "ficha",
            Tipo::Mapa => "mapa",
        }
    }

    /// Tamaño en píxeles: el doble de lo que se muestra en pantalla, para que se vea nítido en
    /// pantallas de alta densidad. Ajusta estos números si quieres más o menos calidad.
    pub fn tamano(self) -> (u32, u32) {
        match self {
            Tipo::Ficha => (128, 128),
            Tipo::Mapa => (256, 144),
        }
    }
}

const REDUCIBLES: [&str; 6] = ["png", "jpg", "jpeg", "webp", "gif", "bmp"];

/// "imagenes/<hash>.png" -> "miniaturas/<tipo>/<hash>.webp". `None` si no es una ruta de imagen válida.
pub fn ruta_miniatura(ruta_imagen: &str, tipo: Tipo) -> Option<String> {
    let nombre = ruta_imagen.strip_prefix("imagenes/")?;
    if nombre.contains('/') || nombre.contains('\\') {
        return None;
    }
    let stem = nombre.rsplit_once('.').map(|(s, _)| s)?;
    (!stem.is_empty()).then(|| format!("miniaturas/{}/{stem}.webp", tipo.carpeta()))
}

/// Devuelve la ruta relativa de la miniatura, generándola si hace falta. Bloqueante (decodifica
/// y reduce una imagen): llamar desde un hilo aparte.
pub fn asegurar(dir_datos: &Path, ruta_imagen: &str, tipo: Tipo) -> Result<Option<String>, String> {
    let origen = ruta_segura(dir_datos, ruta_imagen, &["imagenes"])?;
    let Some(rel) = ruta_miniatura(ruta_imagen, tipo) else { return Ok(None) };

    let destino = dir_datos.join(&rel);
    if destino.is_file() {
        return Ok(Some(rel));
    }

    let extension = origen.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if !REDUCIBLES.contains(&extension.as_str()) || !origen.is_file() {
        return Ok(None);
    }

    let Some(imagen) = decodificar(&origen) else { return Ok(None) };

    let (ancho, alto) = tipo.tamano();
    let reducida = imagen.resize_to_fill(ancho, alto, FilterType::Lanczos3).to_rgba8();
    let webp = webp::Encoder::from_rgba(reducida.as_raw(), reducida.width(), reducida.height()).encode(CALIDAD_WEBP);

    let carpeta = destino.parent().ok_or("Ruta inválida")?;
    fs::create_dir_all(carpeta).map_err(|e| format!("No se pudo crear la carpeta de miniaturas: {e}"))?;

    // Temporal + rename: nunca queda una miniatura a medias con el nombre definitivo.
    let tmp = carpeta.join(format!("{}.tmp", destino.file_name().and_then(|n| n.to_str()).unwrap_or("mini")));
    fs::write(&tmp, &*webp).map_err(|e| format!("No se pudo guardar la miniatura: {e}"))?;
    fs::rename(&tmp, &destino).map_err(|e| format!("No se pudo guardar la miniatura: {e}"))?;

    Ok(Some(rel))
}

/// Decodifica respetando la orientación EXIF (las fotos de móvil), como hace el navegador con la original.
fn decodificar(ruta: &Path) -> Option<DynamicImage> {
    let mut lector = ImageReader::open(ruta).ok()?.with_guessed_format().ok()?;
    // Topes explícitos: una imagen gigantesca no se reduce (se usa la original) en vez de reservar gigas
    let mut limites = image::Limits::default();
    limites.max_image_width = Some(16384);
    limites.max_image_height = Some(16384);
    limites.max_alloc = Some(512 * 1024 * 1024);
    lector.limits(limites);
    let mut decodificador = lector.into_decoder().ok()?;
    let orientacion = decodificador.orientation().ok();
    let mut imagen = DynamicImage::from_decoder(decodificador).ok()?;
    if let Some(o) = orientacion {
        imagen.apply_orientation(o);
    }
    Some(imagen)
}

// ------------------------------------------------------------------
// Miniatura propia de una ficha: la encuadra el usuario en el editor, el front la recorta y manda el
// WebP ya hecho; aquí solo se guarda tal cual. Va por id de ficha (`entidad_<id>_<versión>.webp`) y no
// por hash de la foto, porque dos fichas con la misma foto pueden encuadrarla distinto. La versión
// cambia en cada guardado, así el webview nunca enseña una miniatura vieja de la caché. Quién la usa y
// cuándo se borra lo decide el modelo (`Entidad.miniatura_url`, delta `MiniaturaFichaCambiada`, recursos.rs).
// ------------------------------------------------------------------

/// Lo máximo que se acepta (una miniatura de 128x128 son unos pocos KB).
pub const MAX_BYTES_MINIATURA_FICHA: usize = 512 * 1024;
/// Lado máximo de esa miniatura (el front la hace de 128 px).
const MAX_LADO_MINIATURA_FICHA: u32 = 512;

fn prefijo_de_ficha(id: u16) -> String {
    format!("entidad_{id}_")
}

/// Guarda el WebP de la ficha como archivo nuevo y devuelve su ruta relativa. La anterior no se toca
/// aquí: cuando el delta cambie la ruta de la ficha, recursos.rs borra la que ya no se usa.
pub fn guardar_de_ficha(dir_datos: &Path, id: u16, datos: &[u8]) -> Result<String, String> {
    if datos.len() > MAX_BYTES_MINIATURA_FICHA {
        return Err("La miniatura es demasiado grande".into());
    }
    if datos.len() < 12 || &datos[0..4] != b"RIFF" || &datos[8..12] != b"WEBP" {
        return Err("La miniatura no es un WebP".into());
    }
    // Una cabecera WebP puede decir 16383×16383: quien la pintara reservaría 1 GB. Una miniatura es pequeña.
    match crate::tablero_general::imagenes::dimensiones_de_bytes(datos) {
        Some((w, h)) if w > 0 && h > 0 && w <= MAX_LADO_MINIATURA_FICHA && h <= MAX_LADO_MINIATURA_FICHA => {}
        _ => return Err("La miniatura no es válida o es demasiado grande".into()),
    }
    let carpeta = dir_datos.join("miniaturas/ficha");
    fs::create_dir_all(&carpeta).map_err(|e| format!("No se pudo crear la carpeta de miniaturas: {e}"))?;

    let version = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let nombre = format!("{}{version}.webp", prefijo_de_ficha(id));
    let destino = carpeta.join(&nombre);

    // Temporal + rename: nunca queda una miniatura a medias con el nombre definitivo.
    let tmp = carpeta.join(format!("{nombre}.tmp"));
    fs::write(&tmp, datos).map_err(|e| format!("No se pudo guardar la miniatura: {e}"))?;
    fs::rename(&tmp, &destino).map_err(|e| format!("No se pudo guardar la miniatura: {e}"))?;

    Ok(format!("miniaturas/ficha/{nombre}"))
}

/// Borra las miniaturas de una imagen (todos los tamaños).
pub fn borrar_miniaturas(dir_datos: &Path, ruta_imagen: &str) {
    for tipo in Tipo::TODOS {
        if let Some(rel) = ruta_miniatura(ruta_imagen, tipo) {
            let _ = fs::remove_file(dir_datos.join(rel));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);
    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("vtt_min_{}_{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("imagenes")).unwrap();
        d
    }

    /// 400x200: franja verde a la izquierda (x<50) y el resto rojo. Un recorte centrado no debe incluir el verde.
    fn guardar_imagen(d: &Path, nombre: &str) {
        let mut img = RgbaImage::from_pixel(400, 200, Rgba([200, 0, 0, 255]));
        for y in 0..200 {
            for x in 0..50 {
                img.put_pixel(x, y, Rgba([0, 200, 0, 255]));
            }
        }
        img.save(d.join("imagenes").join(nombre)).unwrap();
    }

    fn leer_webp(d: &Path, rel: &str) -> (Vec<u8>, RgbaImage) {
        let bytes = fs::read(d.join(rel)).unwrap();
        let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::WebP).unwrap().to_rgba8();
        (bytes, img)
    }

    #[test]
    fn genera_un_webp_con_lossy_del_tamano_pedido_y_recortado_sin_deformar() {
        let d = dir();
        guardar_imagen(&d, "abc.png");

        let rel = asegurar(&d, "imagenes/abc.png", Tipo::Ficha).unwrap().unwrap();
        assert_eq!(rel, "miniaturas/ficha/abc.webp");
        let (bytes, img) = leer_webp(&d, &rel);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WEBP");
        assert_eq!(&bytes[12..16], b"VP8 ", "debe ser WebP con pérdida (calidad {CALIDAD_WEBP}), no lossless");
        assert_eq!((img.width(), img.height()), (128, 128));

        let px = img.get_pixel(3, 64);
        assert!(px[0] > 150 && px[1] < 80, "el borde izquierdo debe ser rojo (recorte centrado), era {px:?}; con la imagen estirada saldría verde");

        let rel_mapa = asegurar(&d, "imagenes/abc.png", Tipo::Mapa).unwrap().unwrap();
        assert_eq!(rel_mapa, "miniaturas/mapa/abc.webp");
        assert_eq!(leer_webp(&d, &rel_mapa).1.dimensions(), (256, 144));
    }

    #[test]
    fn la_segunda_peticion_reutiliza_el_archivo() {
        let d = dir();
        guardar_imagen(&d, "abc.png");
        let rel = asegurar(&d, "imagenes/abc.png", Tipo::Ficha).unwrap().unwrap();
        let antes = fs::metadata(d.join(&rel)).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        asegurar(&d, "imagenes/abc.png", Tipo::Ficha).unwrap();
        assert_eq!(fs::metadata(d.join(&rel)).unwrap().modified().unwrap(), antes);
        // y funciona aunque el original ya no esté (la miniatura basta)
        fs::remove_file(d.join("imagenes/abc.png")).unwrap();
        assert_eq!(asegurar(&d, "imagenes/abc.png", Tipo::Ficha).unwrap().unwrap(), rel);
    }

    #[test]
    fn lo_que_no_se_puede_reducir_devuelve_none_para_que_el_front_use_el_original() {
        let d = dir();
        fs::write(d.join("imagenes/vector.svg"), "<svg/>").unwrap();
        fs::write(d.join("imagenes/foto.avif"), "x").unwrap();
        fs::write(d.join("imagenes/rota.png"), "esto no es un png").unwrap();
        for n in ["vector.svg", "foto.avif", "rota.png", "no_existe.png"] {
            assert_eq!(asegurar(&d, &format!("imagenes/{n}"), Tipo::Ficha).unwrap(), None, "{n}");
        }
        assert!(!d.join("miniaturas").exists(), "no se deja nada a medias");
    }

    #[test]
    fn rechaza_rutas_peligrosas_y_calcula_la_ruta_de_la_miniatura() {
        let d = dir();
        for mala in ["../fuera.png", "imagenes/../../x.png", "archivos/entidad_1/a.png", "/etc/passwd", "http://x/y.png"] {
            assert!(asegurar(&d, mala, Tipo::Ficha).is_err(), "{mala}");
        }
        assert_eq!(ruta_miniatura("imagenes/ab12.jpeg", Tipo::Mapa).as_deref(), Some("miniaturas/mapa/ab12.webp"));
        assert_eq!(ruta_miniatura("imagenes/sub/ab12.png", Tipo::Mapa), None);
        assert_eq!(ruta_miniatura("data:image/png;base64,AAA", Tipo::Ficha), None);
        assert_eq!(Tipo::desde_texto("ficha"), Some(Tipo::Ficha));
        assert_eq!(Tipo::desde_texto("otra"), None);
    }

    #[test]
    fn borrar_miniaturas_quita_todos_los_tamanos() {
        let d = dir();
        guardar_imagen(&d, "abc.png");
        asegurar(&d, "imagenes/abc.png", Tipo::Ficha).unwrap();
        asegurar(&d, "imagenes/abc.png", Tipo::Mapa).unwrap();
        borrar_miniaturas(&d, "imagenes/abc.png");
        assert!(!d.join("miniaturas/ficha/abc.webp").exists() && !d.join("miniaturas/mapa/abc.webp").exists());
    }

    /// Un WebP de verdad de 8×8 del color `relleno` (como el que hace el front al recortar).
    fn webp_falso(relleno: u8) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([relleno, 0, 0, 255]));
        webp::Encoder::from_rgba(img.as_raw(), 8, 8).encode_lossless().to_vec()
    }

    /// Solo la cabecera VP8X de un WebP que dice medir `lado`×`lado`.
    fn webp_que_dice(lado: u32) -> Vec<u8> {
        let mut v = b"RIFF".to_vec();
        v.extend_from_slice(&22u32.to_le_bytes());
        v.extend_from_slice(b"WEBPVP8X");
        v.extend_from_slice(&10u32.to_le_bytes());
        v.extend_from_slice(&[0; 4]);
        v.extend_from_slice(&(lado - 1).to_le_bytes()[..3]);
        v.extend_from_slice(&(lado - 1).to_le_bytes()[..3]);
        v
    }

    #[test]
    fn la_miniatura_propia_de_una_ficha_se_guarda_tal_cual_con_una_version_nueva_cada_vez() {
        let d = dir();
        let primera = guardar_de_ficha(&d, 7, &webp_falso(1)).unwrap();
        assert!(primera.starts_with("miniaturas/ficha/entidad_7_") && primera.ends_with(".webp"));
        assert_eq!(fs::read(d.join(&primera)).unwrap(), webp_falso(1));

        // Otra versión: archivo nuevo (el viejo lo libera recursos.rs, no esta función)
        std::thread::sleep(std::time::Duration::from_millis(5));
        let segunda = guardar_de_ficha(&d, 7, &webp_falso(2)).unwrap();
        assert_ne!(primera, segunda);
        assert!(d.join(&primera).exists() && d.join(&segunda).exists());
    }

    #[test]
    fn la_miniatura_propia_rechaza_lo_que_no_es_un_webp_o_es_enorme() {
        let d = dir();
        assert!(guardar_de_ficha(&d, 1, b"esto no es un webp").is_err());
        assert!(guardar_de_ficha(&d, 1, &[]).is_err());
        let mut grande = webp_falso(0);
        grande.resize(MAX_BYTES_MINIATURA_FICHA + 1, 0);
        assert!(guardar_de_ficha(&d, 1, &grande).is_err());
        assert!(guardar_de_ficha(&d, 1, &webp_que_dice(16000)).is_err(), "cabecera de un WebP gigantesco");
        assert!(guardar_de_ficha(&d, 1, b"RIFF\0\0\0\0WEBPbasura").is_err(), "cabecera sin medidas");
        assert!(!d.join("miniaturas").exists(), "no se deja nada");
    }
}
