// src-tauri/src/red/codigo.rs
//
// Los códigos de la conexión directa: el jugador le pasa al host su oferta (SDP de WebRTC) y el host le devuelve
// la respuesta, por los relés de Nostr (`senal.rs`). Un SDP lleva las IPs de cada uno, así que va **cifrado** con
// una clave sacada del secreto del enlace: quien no tenga el enlace ni lo lee ni puede fabricar uno que el host acepte.
//
//   código = base64url( nonce (12 bytes) ‖ AES-256-GCM( deflate( JSON { tipo, sdp, t, n } ) ) )
//   clave  = HKDF-SHA256(secreto, info = "openroll-codigo")
//
// Mismo formato en `src/lib/red/codigo.ts` (con WebCrypto y `CompressionStream("deflate-raw")`); los tests de los dos
// lados descifran un código fijo hecho por el otro.
use std::io::{Read, Write};

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

/// Un código no puede ocupar más (lo que se pega en la ventana de la sala).
pub const MAX_CODIGO: usize = 16 * 1024;
/// Ni descomprimido (un SDP con muchos candidatos ronda los 2–4 KB).
const MAX_CONTENIDO: u64 = 64 * 1024;

/// Lo que va dentro de un código.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Contenido {
    /// "oferta" (del jugador) o "respuesta" (del host).
    pub tipo: String,
    pub sdp: String,
    /// Cuándo se hizo (milisegundos Unix): el host no acepta los viejos.
    pub t: u64,
    /// Aleatorio: el host no acepta dos veces el mismo.
    pub n: String,
}

fn clave(secreto: &str) -> Aes256Gcm {
    let mut k = [0u8; 32];
    Hkdf::<Sha256>::new(None, secreto.as_bytes()).expand(b"openroll-codigo", &mut k).expect("32 bytes caben en HKDF");
    Aes256Gcm::new(&k.into())
}

/// Cifra un contenido con el secreto de la sala (`nonce`, 12 bytes aleatorios; fijo solo en los tests).
pub fn cifrar(secreto: &str, contenido: &Contenido, nonce: [u8; 12]) -> String {
    let json = serde_json::to_vec(contenido).expect("se puede serializar");
    let mut z = DeflateEncoder::new(Vec::new(), Compression::best());
    z.write_all(&json).expect("en memoria");
    let comprimido = z.finish().expect("en memoria");
    let cifrado = clave(secreto).encrypt(Nonce::from_slice(&nonce), comprimido.as_slice()).expect("AES-GCM en memoria");
    let mut todo = nonce.to_vec();
    todo.extend_from_slice(&cifrado);
    URL_SAFE_NO_PAD.encode(todo)
}

/// Descifra un código. `Err` con un motivo corto si no es de esta sala, está dañado o no tiene la forma esperada.
pub fn descifrar(secreto: &str, codigo: &str) -> Result<Contenido, String> {
    let codigo = codigo.trim();
    if codigo.len() > MAX_CODIGO {
        return Err("El código es demasiado largo".into());
    }
    let todo = URL_SAFE_NO_PAD.decode(codigo).map_err(|_| "Eso no es un código válido")?;
    if todo.len() < 12 + 16 {
        return Err("Eso no es un código válido".into());
    }
    let (nonce, cifrado) = todo.split_at(12);
    let comprimido = clave(secreto)
        .decrypt(Nonce::from_slice(nonce), cifrado)
        .map_err(|_| "Ese código no es de esta sala (o está incompleto)")?;
    let mut json = Vec::new();
    DeflateDecoder::new(comprimido.as_slice()).take(MAX_CONTENIDO).read_to_end(&mut json).map_err(|_| "El código está dañado")?;
    serde_json::from_slice(&json).map_err(|_| "El código está dañado".into())
}

/// Milisegundos Unix de ahora.
pub fn ahora_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const SECRETO: &str = "0123456789abcdef_-XYZ";

    pub fn contenido() -> Contenido {
        Contenido { tipo: "oferta".into(), sdp: "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\n".into(), t: 1_700_000_000_000, n: "abc".into() }
    }

    #[test]
    fn ida_y_vuelta_y_solo_con_el_secreto_de_la_sala() {
        let c = cifrar(SECRETO, &contenido(), [7; 12]);
        assert_eq!(descifrar(SECRETO, &c).unwrap(), contenido());
        assert_eq!(descifrar(SECRETO, &format!("  {c}\n")).unwrap(), contenido(), "con espacios al pegar");
        assert!(descifrar("otro-secreto-de-otra-sala", &c).is_err());
        // Cualquier cambio en el código lo invalida (AES-GCM autentica)
        let mut roto: Vec<char> = c.chars().collect();
        let i = roto.len() / 2;
        roto[i] = if roto[i] == 'A' { 'B' } else { 'A' };
        assert!(descifrar(SECRETO, &roto.into_iter().collect::<String>()).is_err());
        assert!(descifrar(SECRETO, "hola").is_err());
        assert!(descifrar(SECRETO, &"A".repeat(MAX_CODIGO + 1)).is_err());
    }

    /// El código fijo que descifra el test de `src/lib/red/codigo.ts` (que no cambie sin cambiar los dos).
    #[test]
    fn el_codigo_fijo_para_el_navegador() {
        let c = cifrar(SECRETO, &contenido(), [7; 12]);
        assert_eq!(c, VECTOR_RUST);
    }

    /// El que hace el navegador (`VECTOR_TS` en tests/codigo.test.ts): Rust lo tiene que poder leer.
    #[test]
    fn descifra_el_codigo_del_navegador() {
        assert_eq!(descifrar(SECRETO, "CQkJCQkJCQkJCQkJu_lSHVLR2Q2YGKJzDEecsZvFdLnZGX6KJUduvMIuO-1LdzBdUUmESrN629q2L9SdYQEX9IJDOEiAInedj5psSC8nto5KPjxup0EijgtyILzHIzFkbso83_bRiP0EmJPy").unwrap(), contenido());
    }

    pub const VECTOR_RUST: &str = "BwcHBwcHBwcHBwcHiYMFR3cAT11gR3LXVL96XEJIQm9JJTRaeHhSaC8i1mmlCqMZz3bulsZDfKBd6lODQ_23Ch58MBYBQ7pINztM4eodFu2-ZG9Lx0XnECOMEP6XY1oXjzWV_KpIaYZBi104leZGxw";
}
