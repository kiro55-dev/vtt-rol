// src-tauri/src/red/pagina.rs
//
// La página de los jugadores va DENTRO de la app (`build-jugador/`, `bun run web:build:app`, con raíz `/`) y la sirve
// el mismo servidor de la sala. Así, con la dirección propia o en red local, el enlace es solo la dirección del host
// (`https://mi.dominio.es/#s=…&k=…`, `http://26.1.2.3:8080/#s=…&k=…`): sin GitHub Pages y con la página siempre de
// la misma versión que el host. Con el túnel de Cloudflare se sigue usando la de GitHub Pages (ver red/config.rs).
//
// En desarrollo rust-embed lee la carpeta del disco; en release la lleva dentro del ejecutable.
use axum::http::{header, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../build-jugador"]
struct Pagina;

fn tipo(ruta: &str) -> &'static str {
    match ruta.rsplit('.').next().map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Lo que no es `/ws` ni `/imagen`: un archivo de la página, o `index.html` para `/` (la app es una sola página).
pub async fn servir(uri: Uri) -> Response {
    let pedida = uri.path().trim_start_matches('/');
    let ruta = if pedida.is_empty() { "index.html" } else { pedida };
    let Some(archivo) = Pagina::get(ruta) else {
        if Pagina::get("index.html").is_none() {
            // La app se compiló sin construir antes la página (`bun run web:build:app`)
            return (StatusCode::SERVICE_UNAVAILABLE, "Falta compilar la página de los jugadores (bun run web:build:app).").into_response();
        }
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut r = archivo.data.into_owned().into_response();
    let c = r.headers_mut();
    c.insert(header::CONTENT_TYPE, HeaderValue::from_static(tipo(ruta)));
    // Lo de `_app/immutable/` lleva el hash en el nombre: nunca cambia. Lo demás (index.html, el Service Worker) se
    // revisa siempre, para que una página vieja guardada no cargue piezas que ya no existen («pantalla negra»).
    let cache = if ruta.starts_with("_app/immutable/") { "public, max-age=31536000, immutable" } else { "no-cache" };
    c.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    c.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    c.insert("x-frame-options", HeaderValue::from_static("DENY"));
    c.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    r
}

/// Si la app lleva la página (para avisar en la ventana de la sala si no).
pub fn incluida() -> bool {
    Pagina::get("index.html").is_some()
}
