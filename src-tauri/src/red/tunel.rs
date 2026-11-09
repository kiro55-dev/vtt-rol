// src-tauri/src/red/tunel.rs
//
// El túnel que publica el servidor local de la sala en una dirección https temporal, sin cuenta:
// `cloudflared tunnel --url http://127.0.0.1:<puerto>` (Cloudflare «quick tunnel»), que da una dirección
// `https://<palabras>.trycloudflare.com` distinta cada vez.
//
// cloudflared no va dentro del instalador: se descarga la primera vez que se abre una sala, de las releases
// oficiales de Cloudflare en GitHub, a `<datos de la app>/bin/`. La versión está FIJADA y su SHA-256 se
// comprueba antes de usarlo: si no coincide, no se ejecuta. Para actualizarlo, cambia `VERSION` y las
// huellas (las da la API de releases de GitHub, campo `digest`).
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

pub const VERSION: &str = "2026.10.7";

/// (nombre del archivo en la release, SHA-256) para esta plataforma. `None` si no hay cloudflared para ella.
fn descarga() -> Option<(&'static str, &'static str)> {
    let d = |n, h| Some((n, h));
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        d("cloudflared-windows-amd64.exe", "2493dbc168923bb3e59e5e7716f71deced92992eea9acd6e9d74f84ac7751343")
    } else if cfg!(all(target_os = "windows", target_arch = "x86")) {
        d("cloudflared-windows-386.exe", "88c6b972b410db0e26745f593510d772c825a13c5a2374819e3f13a05b333082")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        d("cloudflared-linux-amd64", "1f99fb518b83dec2ba8d870a7cc991d82a77e6196295a3d2754ba17ab9f91872")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        d("cloudflared-linux-arm64", "3df43aedc9ff291d1735b5cb576cb97e0b157488796db2e32b28aecabb445775")
    } else if cfg!(all(target_os = "linux", target_arch = "arm")) {
        d("cloudflared-linux-arm", "74f690f6707182839b63e0465726c7776dd63f0a174390b911d5d71f35cab94e")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        d("cloudflared-darwin-amd64.tgz", "02f3edfd87684dd5b705607688e4eb4b323f6e777b81e7b3a85af556aa356848")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        d("cloudflared-darwin-arm64.tgz", "3586ccf397e98e5f437e60e7c307ca6ac463e110580503af8bb8e6f9ebf831bd")
    } else {
        None
    }
}

fn nombre_binario() -> String {
    format!("cloudflared-{VERSION}{}", if cfg!(windows) { ".exe" } else { "" })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// La ruta de cloudflared, descargándolo si aún no está. `progreso(bytes, total)` se llama durante la descarga.
pub async fn asegurar_cloudflared(raiz: &Path, progreso: impl Fn(u64, Option<u64>)) -> Result<PathBuf, String> {
    let carpeta = raiz.join("bin");
    let binario = carpeta.join(nombre_binario());
    if binario.is_file() {
        return Ok(binario);
    }
    let (archivo, huella) = descarga().ok_or("No hay cloudflared para este sistema")?;
    tokio::fs::create_dir_all(&carpeta).await.map_err(|e| format!("No se pudo crear {}: {e}", carpeta.display()))?;

    let url = format!("https://github.com/cloudflare/cloudflared/releases/download/{VERSION}/{archivo}");
    let respuesta = reqwest::get(&url)
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("No se pudo descargar cloudflared: {e}"))?;
    let total = respuesta.content_length();
    let tmp = carpeta.join(format!("{archivo}.descargando"));
    let mut destino = tokio::fs::File::create(&tmp).await.map_err(|e| format!("No se pudo guardar cloudflared: {e}"))?;
    let mut hash = Sha256::new();
    let mut hecho = 0u64;
    let mut ultimo_aviso = 0u64;
    let mut trozos = respuesta.bytes_stream();
    while let Some(trozo) = trozos.next().await {
        let trozo = trozo.map_err(|e| format!("Se cortó la descarga de cloudflared: {e}"))?;
        hash.update(&trozo);
        destino.write_all(&trozo).await.map_err(|e| format!("No se pudo guardar cloudflared: {e}"))?;
        hecho += trozo.len() as u64;
        if hecho - ultimo_aviso >= 512 * 1024 {
            ultimo_aviso = hecho;
            progreso(hecho, total);
        }
    }
    destino.flush().await.map_err(|e| e.to_string())?;
    drop(destino);

    if hex(&hash.finalize()) != huella {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err("La descarga de cloudflared no coincide con la oficial: no se usará".into());
    }
    if archivo.ends_with(".tgz") {
        extraer_tgz(&tmp, &binario)?;
        let _ = tokio::fs::remove_file(&tmp).await;
    } else {
        tokio::fs::rename(&tmp, &binario).await.map_err(|e| format!("No se pudo guardar cloudflared: {e}"))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&binario, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    Ok(binario)
}

#[cfg(target_os = "macos")]
fn extraer_tgz(tgz: &Path, destino: &Path) -> Result<(), String> {
    let archivo = std::fs::File::open(tgz).map_err(|e| e.to_string())?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archivo));
    for entrada in tar.entries().map_err(|e| e.to_string())? {
        let mut entrada = entrada.map_err(|e| e.to_string())?;
        let es_binario = entrada.path().ok().and_then(|p| p.file_name().map(|n| n == "cloudflared")).unwrap_or(false);
        if es_binario {
            entrada.unpack(destino).map_err(|e| e.to_string())?;
            return Ok(());
        }
    }
    Err("El paquete de cloudflared no trae el programa".into())
}

#[cfg(not(target_os = "macos"))]
fn extraer_tgz(_tgz: &Path, _destino: &Path) -> Result<(), String> {
    Err("Paquete .tgz solo esperado en macOS".into())
}

/// Saca `xxxx.trycloudflare.com` de una línea de la salida de cloudflared.
pub fn direccion_en(linea: &str) -> Option<String> {
    let inicio = linea.find("https://")? + "https://".len();
    let resto = &linea[inicio..];
    let fin = resto.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '.')).unwrap_or(resto.len());
    let host = &resto[..fin];
    (host.ends_with(".trycloudflare.com") && host != "api.trycloudflare.com").then(|| host.to_string())
}

/// Lanza el túnel hacia `127.0.0.1:<puerto>` y espera a que cloudflared diga su dirección pública.
/// El proceso muere al soltar el `Child` (`kill_on_drop`), así que cerrar la sala o la app lo para.
pub async fn lanzar(binario: &Path, puerto: u16) -> Result<(Child, String), String> {
    let mut orden = Command::new(binario);
    orden
        .args(["tunnel", "--no-autoupdate", "--url", &format!("http://127.0.0.1:{puerto}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    orden.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: sin ventana de consola
    let mut proceso = orden.spawn().map_err(|e| format!("No se pudo arrancar cloudflared: {e}"))?;
    let salida = proceso.stderr.take().ok_or("cloudflared no da salida")?;
    let mut lineas = BufReader::new(salida).lines();

    let encontrada = tokio::time::timeout(Duration::from_secs(45), async {
        while let Ok(Some(linea)) = lineas.next_line().await {
            if let Some(d) = direccion_en(&linea) {
                return Some(d);
            }
        }
        None
    })
    .await;
    let direccion = match encontrada {
        Ok(Some(d)) => d,
        Ok(None) => return Err("cloudflared se cerró sin abrir el túnel (¿hay conexión a internet?)".into()),
        Err(_) => return Err("cloudflared tarda demasiado en abrir el túnel".into()),
    };
    // Se sigue leyendo su salida para que no se llene la tubería y se bloquee
    tokio::spawn(async move { while let Ok(Some(_)) = lineas.next_line().await {} });
    Ok((proceso, direccion))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_direccion_se_lee_de_la_salida_de_cloudflared() {
        let l = "2026-11-09T10:00:00Z INF |  https://amigo-casa-roja-luna.trycloudflare.com                      |";
        assert_eq!(direccion_en(l).as_deref(), Some("amigo-casa-roja-luna.trycloudflare.com"));
        assert_eq!(direccion_en("INF Requesting new quick Tunnel on trycloudflare.com..."), None);
        assert_eq!(direccion_en("ERR https://api.trycloudflare.com/tunnel failed"), None);
        assert_eq!(direccion_en("https://evil.example.com"), None);
    }

    #[test]
    fn hay_descarga_fijada_para_esta_plataforma() {
        let (archivo, huella) = descarga().expect("plataforma soportada");
        assert!(archivo.starts_with("cloudflared-") && huella.len() == 64);
    }
}
