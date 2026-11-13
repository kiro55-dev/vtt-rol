// src-tauri/src/red/version.rs
//
// ¿Los jugadores van a poder entrar? Con el túnel de Cloudflare la página de los jugadores es la de GitHub Pages, que se
// actualiza sola al subir cambios, mientras que la app del host no: si la web trae un protocolo más nuevo que la app, los
// jugadores verían «versión distinta» y no podrían entrar. La web publica un `version.json` (`src/routes/version.json`,
// con el protocolo y la versión de la app con que se compiló) y el host lo consulta al abrir la sala para avisar.
//
// Solo se pregunta a la propia web de los jugadores (`URL_CLIENTE`), una vez al abrir la sala con Cloudflare. Con tu propia
// dirección o en red local la página va dentro de la app y no puede haber desajuste: no se consulta nada. Si no hay
// internet o la web no contesta, no se avisa de nada (no es un error).
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::red::protocolo::VERSION_PROTOCOLO;

/// Lo que publica la web (`version.json`).
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct VersionWeb {
    pub protocolo: u32,
    /// La versión de la app con que se compiló la web (`x.y.z`).
    #[serde(default)]
    pub app: String,
}

/// Lo que se le enseña al host.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct AvisoVersion {
    /// `"fuerte"`: los jugadores NO podrán entrar hasta que actualice. `"suave"`: hay una versión nueva, pero entran.
    pub nivel: &'static str,
    pub texto: String,
}

/// Lo máximo que se lee del `version.json` (son unas decenas de bytes).
const MAX_BYTES: usize = 4096;
const TIEMPO_MAX: Duration = Duration::from_secs(6);

/// Los números de una versión `x.y.z` (lo que no es número cuenta como 0; faltan partes = 0).
fn numeros(v: &str) -> [u64; 3] {
    let mut n = [0u64; 3];
    for (i, parte) in v.trim().trim_start_matches('v').split('.').take(3).enumerate() {
        n[i] = parte.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0);
    }
    n
}

/// ¿`a` es una versión más nueva que `b`?
pub fn mas_nueva(a: &str, b: &str) -> bool {
    numeros(a) > numeros(b)
}

/// Qué decirle al host dados mi protocolo y mi versión y lo que publica la web. `None` si no hay nada que avisar (iguales, o
/// la web va por detrás de la app: eso es cosa de quien publica, no del host).
pub fn evaluar(mi_protocolo: u32, mi_app: &str, web: &VersionWeb) -> Option<AvisoVersion> {
    if web.protocolo > mi_protocolo {
        let nueva = if web.app.is_empty() { String::new() } else { format!(" (versión {})", web.app) };
        return Some(AvisoVersion {
            nivel: "fuerte",
            texto: format!(
                "Hay una versión más nueva de OpenRoll{nueva} y la web de los jugadores ya la usa: con la tuya (versión {mi_app}) no podrán entrar. Actualiza la app."
            ),
        });
    }
    if !web.app.is_empty() && mas_nueva(&web.app, mi_app) {
        return Some(AvisoVersion {
            nivel: "suave",
            texto: format!("Hay una versión nueva de OpenRoll ({}); tú tienes la {mi_app}. Puedes actualizar cuando quieras.", web.app),
        });
    }
    None
}

/// Descarga y lee un `version.json`. `Err` si no hay red, no contesta, es demasiado grande o no es lo esperado.
pub async fn consultar(url: &str) -> Result<VersionWeb, String> {
    let cliente = reqwest::Client::builder().timeout(TIEMPO_MAX).build().map_err(|e| e.to_string())?;
    let respuesta = cliente.get(url).send().await.map_err(|e| e.to_string())?;
    if !respuesta.status().is_success() {
        return Err(format!("la web contestó {}", respuesta.status()));
    }
    if respuesta.content_length().is_some_and(|n| n as usize > MAX_BYTES) {
        return Err("version.json demasiado grande".into());
    }
    let bytes = respuesta.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() > MAX_BYTES {
        return Err("version.json demasiado grande".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("version.json no es válido: {e}"))
}

/// El `version.json` de la web de los jugadores (junto a su página).
pub fn url_version(url_cliente: &str) -> String {
    format!("{}/version.json", url_cliente.trim_end_matches('/'))
}

/// Consulta la web de los jugadores y dice si hay algo que avisar. Cualquier fallo de red: `None`.
pub async fn comprobar(url_cliente: &str) -> Option<AvisoVersion> {
    let web = consultar(&url_version(url_cliente)).await.ok()?;
    evaluar(VERSION_PROTOCOLO, env!("CARGO_PKG_VERSION"), &web)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn web(protocolo: u32, app: &str) -> VersionWeb {
        VersionWeb { protocolo, app: app.into() }
    }

    #[test]
    fn las_versiones_se_comparan_por_numeros_no_por_texto() {
        assert!(mas_nueva("1.0.2", "1.0.1"));
        assert!(mas_nueva("1.10.0", "1.9.9"), "10 > 9 (por texto saldría al revés)");
        assert!(mas_nueva("2.0.0", "1.99.99"));
        assert!(mas_nueva("v1.0.1", "1.0.0"), "con v delante");
        assert!(!mas_nueva("1.0.1", "1.0.1"));
        assert!(!mas_nueva("1.0.0", "1.0.1"));
        assert!(!mas_nueva("basura", "1.0.0") && mas_nueva("1.0.0", "basura"), "lo que no es número cuenta como 0");
        assert!(mas_nueva("1.1", "1.0.9"), "faltan partes = 0");
        assert!(!mas_nueva("1.0.1-beta", "1.0.1"), "una etiqueta detrás no cambia el número");
    }

    #[test]
    fn la_web_con_un_protocolo_mas_nuevo_es_un_aviso_fuerte_y_con_solo_otra_version_suave() {
        let fuerte = evaluar(9, "1.0.1", &web(10, "1.1.0")).unwrap();
        assert_eq!(fuerte.nivel, "fuerte");
        assert!(fuerte.texto.contains("no podrán entrar") && fuerte.texto.contains("1.1.0") && fuerte.texto.contains("1.0.1"), "{}", fuerte.texto);
        // Aunque la versión de la app no diga nada, el protocolo manda
        assert_eq!(evaluar(9, "1.0.1", &web(10, "")).unwrap().nivel, "fuerte");

        let suave = evaluar(10, "1.0.1", &web(10, "1.0.2")).unwrap();
        assert_eq!(suave.nivel, "suave");
        assert!(suave.texto.contains("1.0.2") && suave.texto.contains("1.0.1"));
    }

    #[test]
    fn si_esta_al_dia_o_la_web_va_por_detras_no_se_avisa_de_nada() {
        assert_eq!(evaluar(10, "1.0.1", &web(10, "1.0.1")), None);
        assert_eq!(evaluar(10, "1.0.1", &web(10, "")), None);
        assert_eq!(evaluar(10, "1.0.2", &web(10, "1.0.1")), None, "mi app es más nueva que la web: cosa de quien publica");
        assert_eq!(evaluar(10, "1.0.1", &web(9, "1.0.0")), None, "protocolo más viejo en la web: no es del host");
    }

    #[test]
    fn la_direccion_del_version_json_cuelga_de_la_pagina() {
        assert_eq!(url_version("https://a.github.io/OpenRoll/"), "https://a.github.io/OpenRoll/version.json");
        assert_eq!(url_version("http://localhost:5173"), "http://localhost:5173/version.json");
    }

    // ---- Con una web de mentira ----
    use axum::routing::get;
    use axum::Router;

    async fn web_de_mentira(cuerpo: &'static str, estado: axum::http::StatusCode) -> String {
        let app = Router::new().route("/version.json", get(move || async move { (estado, cuerpo) }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let puerto = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("http://127.0.0.1:{puerto}/")
    }

    #[tokio::test]
    async fn se_lee_el_version_json_de_la_web_y_se_avisa_solo_si_hace_falta() {
        let url = web_de_mentira(r#"{"protocolo": 99, "app": "9.9.9"}"#, axum::http::StatusCode::OK).await;
        assert_eq!(consultar(&url_version(&url)).await.unwrap(), web(99, "9.9.9"));
        let aviso = comprobar(&url).await.expect("la web va por delante");
        assert_eq!(aviso.nivel, "fuerte");

        // Una web igual que la app: nada
        let igual = format!(r#"{{"protocolo": {VERSION_PROTOCOLO}, "app": "{}"}}"#, env!("CARGO_PKG_VERSION"));
        let url = web_de_mentira(Box::leak(igual.into_boxed_str()), axum::http::StatusCode::OK).await;
        assert_eq!(comprobar(&url).await, None);
    }

    #[tokio::test]
    async fn lo_que_no_es_un_version_json_valido_no_avisa_ni_falla() {
        for (cuerpo, estado) in [
            ("no es json", axum::http::StatusCode::OK),
            (r#"{"app": "1.0.0"}"#, axum::http::StatusCode::OK), // sin protocolo
            (r#"{"protocolo": 99}"#, axum::http::StatusCode::NOT_FOUND),
        ] {
            let url = web_de_mentira(cuerpo, estado).await;
            assert!(consultar(&url_version(&url)).await.is_err(), "{cuerpo}");
            assert_eq!(comprobar(&url).await, None, "{cuerpo}");
        }
        // Demasiado grande
        let enorme: &'static str = Box::leak(format!(r#"{{"protocolo": 99, "app": "{}"}}"#, "x".repeat(MAX_BYTES)).into_boxed_str());
        let url = web_de_mentira(enorme, axum::http::StatusCode::OK).await;
        assert!(consultar(&url_version(&url)).await.is_err());
        // Sin nadie escuchando (sin red): tampoco avisa
        assert_eq!(comprobar("http://127.0.0.1:1/").await, None);
    }
}
