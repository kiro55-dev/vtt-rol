// src-tauri/src/red/config.rs
//
// Cómo se publica la sala en internet, elegido por el host en «Configuración de sala». Es de ESTE equipo, no de la
// partida: vive en `<datos de la app>/red.json` (aparte de `config.json`, que `partidas.rs` reescribe entero).
//
//  * `Cloudflare` (por defecto): un túnel rápido de Cloudflare, sin cuenta ni configuración (`tunel.rs`). La
//    dirección cambia cada vez que se abre la sala.
//  * `Propio`: el host ya tiene su propia forma de llegar a su equipo (un dominio con su túnel de Cloudflare,
//    `tailscale funnel`, `ngrok`, un puerto abierto con un proxy https...). La app escucha en `127.0.0.1:<puerto>`
//    y el enlace es esa dirección https: la página la sirve la propia app (red/pagina.rs), sin GitHub Pages.
//  * `Lan`: como en los juegos «abrir en LAN». La app escucha en TODAS las interfaces (`0.0.0.0:<puerto>`) y hay un
//    enlace `http://<ip>:<puerto>/` por cada IP del equipo (la de la red de casa, la de Radmin VPN, Hamachi,
//    ZeroTier...). Sin internet ni servicios de nadie, pero SIN CIFRAR: solo para redes de confianza.
// (En la fase 3, con la conexión directa por WebRTC, se añadirá `Ninguno`: solo directo.)
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

const ARCHIVO: &str = "red.json";
const ARCHIVO_TMP: &str = "red.json.tmp";
/// Puertos que se aceptan para escuchar (por debajo de 1024 hacen falta permisos de administrador).
pub const PUERTO_MIN: u16 = 1024;
const MAX_DIRECCION: usize = 253;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(tag = "tipo")]
pub enum Respaldo {
    #[default]
    Cloudflare,
    /// `url`: la dirección https por la que llegan los jugadores (`https://mi.dominio.es`, con puerto si hace
    /// falta). `puerto`: donde escucha la app en este equipo, al que esa dirección debe llevar.
    Propio { url: String, puerto: u16 },
    /// En la red local o una VPN de juego, en `puerto` de todas las interfaces.
    Lan { puerto: u16 },
}

impl Respaldo {
    /// Si la página de los jugadores la sirve la propia app (si no, la de GitHub Pages).
    pub fn sirve_pagina(&self) -> bool {
        !matches!(self, Respaldo::Cloudflare)
    }
}

/// Una dirección de este equipo en la red local o en una VPN: el nombre de la interfaz (p. ej. «Radmin VPN») y su IP.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct DireccionLan {
    pub interfaz: String,
    pub ip: std::net::Ipv4Addr,
}

/// Las IPv4 de este equipo por las que pueden llegar los jugadores en red local (sin la de bucle local ni las
/// autoasignadas 169.254.x.x, que no llevan a ningún sitio).
pub fn direcciones_lan() -> Vec<DireccionLan> {
    let mut v: Vec<DireccionLan> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|i| match i.ip() {
            std::net::IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_link_local() && !ip.is_unspecified() => {
                Some(DireccionLan { interfaz: i.name.clone(), ip })
            }
            _ => None,
        })
        .collect();
    v.sort_by_key(|d| d.ip);
    v.dedup_by_key(|d| d.ip);
    v
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct ConfigRed {
    #[serde(default)]
    pub respaldo: Respaldo,
}

/// La de este equipo; la de serie si no hay o está rota (no se puede quedar sin poder abrir la sala).
pub fn leer(raiz: &Path) -> ConfigRed {
    fs::read_to_string(raiz.join(ARCHIVO))
        .ok()
        .and_then(|t| serde_json::from_str::<ConfigRed>(&t).ok())
        .filter(|c| validar(c).is_ok())
        .unwrap_or_default()
}

/// Guarda la configuración, si es válida. Escritura atómica (temporal + rename).
pub fn guardar(raiz: &Path, config: &ConfigRed) -> Result<(), String> {
    validar(config)?;
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::create_dir_all(raiz).map_err(|e| e.to_string())?;
    let tmp = raiz.join(ARCHIVO_TMP);
    fs::write(&tmp, json).map_err(|e| format!("No se pudo guardar la configuración de red: {e}"))?;
    fs::rename(&tmp, raiz.join(ARCHIVO)).map_err(|e| format!("No se pudo guardar la configuración de red: {e}"))
}

pub fn validar(config: &ConfigRed) -> Result<(), String> {
    let puerto_ok = |p: u16| {
        if p < PUERTO_MIN { Err(format!("El puerto tiene que estar entre {PUERTO_MIN} y 65535")) } else { Ok(()) }
    };
    match &config.respaldo {
        Respaldo::Cloudflare => Ok(()),
        Respaldo::Propio { url, puerto } => {
            puerto_ok(*puerto)?;
            direccion_de_url(url).map(|_| ())
        }
        Respaldo::Lan { puerto } => puerto_ok(*puerto),
    }
}

/// De la URL que escribe el host, la dirección que va en el enlace: `host` o `host:puerto`, en minúsculas. El
/// navegador del jugador le añade `wss://…/ws` y `https://…/imagen`, así que solo se admite https y sin ruta.
pub fn direccion_de_url(url: &str) -> Result<String, String> {
    let url = url.trim();
    let resto = if let Some(r) = url.strip_prefix("https://") {
        r
    } else if url.starts_with("http://") {
        return Err("Tiene que ser https:// (los navegadores no dejan conectar sin cifrar desde la página)".into());
    } else if url.contains("://") {
        return Err("Tiene que empezar por https://".into());
    } else {
        url
    };
    let direccion = resto.strip_suffix('/').unwrap_or(resto).to_ascii_lowercase();
    if direccion.is_empty() || direccion.len() > MAX_DIRECCION {
        return Err("Escribe la dirección, p. ej. https://mi.dominio.es".into());
    }
    if direccion.contains(['/', '?', '#', '@', ' ', '\\']) {
        return Err("Solo la dirección, sin rutas ni nada más (p. ej. https://mi.dominio.es)".into());
    }
    let (host, puerto) = match direccion.rsplit_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (direccion.as_str(), None),
    };
    if let Some(p) = puerto {
        if p.parse::<u16>().ok().filter(|p| *p > 0).is_none() {
            return Err("El puerto de la dirección no es válido".into());
        }
    }
    let etiqueta_ok = |e: &str| {
        !e.is_empty() && e.len() <= 63 && !e.starts_with('-') && !e.ends_with('-') && e.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    };
    if !host.split('.').all(etiqueta_ok) {
        return Err("La dirección no es válida".into());
    }
    Ok(direccion)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tablero_general::almacen::pruebas::dir_temporal;

    #[test]
    fn la_direccion_propia_solo_admite_https_sin_ruta() {
        assert_eq!(direccion_de_url(" https://Mi.Dominio.es/ ").unwrap(), "mi.dominio.es");
        assert_eq!(direccion_de_url("https://mi.dominio.es:8443").unwrap(), "mi.dominio.es:8443");
        assert_eq!(direccion_de_url("mesa.tail1234.ts.net").unwrap(), "mesa.tail1234.ts.net", "sin esquema: se da por https");
        for mala in [
            "http://mi.dominio.es",
            "ftp://mi.dominio.es",
            "https://mi.dominio.es/sala",
            "https://usuario@mi.dominio.es",
            "https://mi dominio.es",
            "https://mi.dominio.es:99999",
            "https://mi.dominio.es:0",
            "https://-malo.es",
            "https://",
            "",
        ] {
            assert!(direccion_de_url(mala).is_err(), "{mala:?}");
        }
    }

    #[test]
    fn se_guarda_y_se_lee_y_si_falta_o_esta_rota_vale_la_de_serie() {
        let raiz = dir_temporal("config_red");
        assert_eq!(leer(&raiz), ConfigRed::default(), "sin archivo: Cloudflare");
        let propia = ConfigRed { respaldo: Respaldo::Propio { url: "https://mi.dominio.es".into(), puerto: 8080 } };
        guardar(&raiz, &propia).unwrap();
        assert_eq!(leer(&raiz), propia);
        assert!(guardar(&raiz, &ConfigRed { respaldo: Respaldo::Propio { url: "http://x.es".into(), puerto: 8080 } }).is_err());
        assert!(guardar(&raiz, &ConfigRed { respaldo: Respaldo::Propio { url: "https://x.es".into(), puerto: 80 } }).is_err(), "puerto bajo");
        assert!(guardar(&raiz, &ConfigRed { respaldo: Respaldo::Lan { puerto: 80 } }).is_err(), "puerto bajo");
        assert_eq!(leer(&raiz), propia, "lo que no vale no se guarda");
        std::fs::write(raiz.join(ARCHIVO), "{ roto").unwrap();
        assert_eq!(leer(&raiz), ConfigRed::default());
        // El formato que se guarda
        let json = serde_json::to_string(&propia).unwrap();
        assert_eq!(json, r#"{"respaldo":{"tipo":"Propio","url":"https://mi.dominio.es","puerto":8080}}"#);
        let lan = ConfigRed { respaldo: Respaldo::Lan { puerto: 8080 } };
        guardar(&raiz, &lan).unwrap();
        assert_eq!(leer(&raiz), lan);
        assert_eq!(serde_json::to_string(&lan).unwrap(), r#"{"respaldo":{"tipo":"Lan","puerto":8080}}"#);
    }

    #[test]
    fn las_direcciones_lan_no_incluyen_la_de_bucle_local() {
        // Las que haya en este equipo: ninguna es 127.x ni 169.254.x, y no se repiten
        let d = direcciones_lan();
        assert!(d.iter().all(|d| !d.ip.is_loopback() && !d.ip.is_link_local()));
        let mut ips: Vec<_> = d.iter().map(|d| d.ip).collect();
        ips.dedup();
        assert_eq!(ips.len(), d.len());
    }
}
