// src-tauri/src/red/sala.rs
//
// La identidad de la sala de UNA partida: lo que hace válido un enlace, la contraseña y a quién se
// recuerda. Vive en `<partida>/sala.json` y es SOLO del host: no está en `EstadoPartida` ni en el log de
// deltas, así que nunca sale por la red ni se reproduce.
//
//  * `id` y `secreto` van en el enlace (`#s=…&k=…`, detrás del `#`, que el navegador nunca envía a nadie).
//    Quien no los tenga no pasa del saludo.
//  * La contraseña (opcional) se guarda como hash Argon2id.
//  * Al entrar por primera vez, el jugador recibe un token aleatorio que su navegador guarda. Aquí solo se
//    guarda su clave (SHA-256 del token), nunca el token.
//  * El token NUNCA vuelve a viajar: para entrar sin contraseña, el navegador responde a un reto aleatorio
//    con HMAC-SHA256(clave, reto | dirección del túnel). Así, un enlace falso que lleve a otro servidor
//    no puede robar nada: la respuesta solo vale para ese reto y esa dirección.
//  * `regenerar` cambia `id` y `secreto` y olvida todos los tokens: el enlace viejo deja de valer y todos
//    vuelven a poner la contraseña. Los jugadores siguen registrados en la partida, con sus fichas.
use std::fs;
use std::path::Path;

use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::Argon2;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::tablero_general::modelos::IdJugador;

const ARCHIVO: &str = "sala.json";
const ARCHIVO_TMP: &str = "sala.json.tmp";
/// Dispositivos recordados por jugador (navegador del PC, del móvil...). Al pasarse, se olvida el más viejo.
const MAX_TOKENS_POR_JUGADOR: usize = 8;
pub const MAX_CONTRASENA: usize = 128;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TokenJugador {
    pub id_jugador: IdJugador,
    /// Clave del navegador: SHA-256 del token, en hexadecimal.
    pub hash: String,
}

/// HMAC-SHA256(clave, "<reto>|<túnel>") en hexadecimal: lo que calcula el navegador para entrar sin
/// contraseña (cliente-web/src/conexion.svelte.ts hace lo mismo con WebCrypto).
pub fn prueba(clave: &[u8], reto: &str, tunel: &str) -> String {
    use hmac::{Hmac, Mac};
    let mut mac = Hmac::<Sha256>::new_from_slice(clave).expect("HMAC acepta cualquier clave");
    mac.update(format!("{reto}|{tunel}").as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Un reto nuevo para un saludo.
pub fn reto() -> String {
    aleatorio::<32>()
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SalaGuardada {
    pub id: String,
    pub secreto: String,
    /// PHC de Argon2id. `None` = sin contraseña: entra cualquiera con el enlace.
    #[serde(default)]
    pub hash_contrasena: Option<String>,
    #[serde(default)]
    pub tokens: Vec<TokenJugador>,
}

/// Texto aleatorio para enlaces y tokens (base64 de URL, sin relleno).
fn aleatorio<const N: usize>() -> String {
    URL_SAFE_NO_PAD.encode(rand::random::<[u8; N]>())
}

fn sha256_hex(texto: &str) -> String {
    Sha256::digest(texto.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Igualdad en tiempo constante (no revela por el tiempo cuántos caracteres acertó quien prueba).
pub(crate) fn iguales(a: &str, b: &str) -> bool {
    a.len() == b.len() && bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}

impl SalaGuardada {
    pub fn nueva() -> Self {
        SalaGuardada { id: aleatorio::<16>(), secreto: aleatorio::<16>(), hash_contrasena: None, tokens: Vec::new() }
    }

    /// La de la carpeta de la partida, o una nueva (que se guarda) si no hay o está rota.
    pub fn leer_o_crear(dir: &Path) -> Result<Self, String> {
        match fs::read_to_string(dir.join(ARCHIVO)) {
            Ok(texto) => match serde_json::from_str(&texto) {
                Ok(sala) => return Ok(sala),
                // Rota: se empieza otra. El enlace viejo deja de valer, que es lo seguro.
                Err(e) => eprintln!("[red] sala.json ilegible ({e}); se crea una sala nueva"),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("No se pudo leer sala.json: {e}")),
        }
        let sala = SalaGuardada::nueva();
        sala.guardar(dir)?;
        Ok(sala)
    }

    /// Escritura atómica (archivo temporal + rename), como `partida.json`.
    pub fn guardar(&self, dir: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| format!("No se pudo preparar sala.json: {e}"))?;
        let tmp = dir.join(ARCHIVO_TMP);
        fs::write(&tmp, json).map_err(|e| format!("No se pudo escribir sala.json: {e}"))?;
        fs::rename(&tmp, dir.join(ARCHIVO)).map_err(|e| format!("No se pudo escribir sala.json: {e}"))
    }

    /// Enlace nuevo: el anterior deja de valer y nadie queda recordado. La contraseña se conserva.
    pub fn regenerar(&mut self) {
        self.id = aleatorio::<16>();
        self.secreto = aleatorio::<16>();
        self.tokens.clear();
    }

    /// `id` y `secreto` del enlace que trae un jugador.
    pub fn coincide(&self, id: &str, secreto: &str) -> bool {
        // Las dos comparaciones siempre, para no dar pistas por el tiempo
        let a = iguales(&self.id, id);
        let b = iguales(&self.secreto, secreto);
        a & b
    }

    // ------------------------------------------------------------------
    // Contraseña
    // ------------------------------------------------------------------

    pub fn pide_contrasena(&self) -> bool {
        self.hash_contrasena.is_some()
    }

    /// `None` o vacía = sin contraseña.
    pub fn poner_contrasena(&mut self, contrasena: Option<&str>) -> Result<(), String> {
        let Some(c) = contrasena.filter(|c| !c.is_empty()) else {
            self.hash_contrasena = None;
            return Ok(());
        };
        if c.chars().count() > MAX_CONTRASENA {
            return Err(format!("La contraseña no puede pasar de {MAX_CONTRASENA} caracteres"));
        }
        let hash = Argon2::default()
            .hash_password(c.as_bytes())
            .map_err(|e| format!("No se pudo guardar la contraseña: {e}"))?;
        self.hash_contrasena = Some(hash.to_string());
        Ok(())
    }

    /// Sin contraseña puesta, vale cualquier cosa (también ninguna).
    pub fn comprobar_contrasena(&self, contrasena: Option<&str>) -> bool {
        let Some(guardada) = &self.hash_contrasena else { return true };
        let (Some(c), Ok(hash)) = (contrasena, PasswordHash::new(guardada)) else { return false };
        c.chars().count() <= MAX_CONTRASENA && Argon2::default().verify_password(c.as_bytes(), &hash).is_ok()
    }

    // ------------------------------------------------------------------
    // Jugadores recordados
    // ------------------------------------------------------------------

    /// Token nuevo para el navegador de `id_jugador` (se devuelve en claro UNA vez; aquí queda su hash).
    pub fn nuevo_token(&mut self, id_jugador: IdJugador) -> String {
        let token = aleatorio::<32>();
        self.tokens.push(TokenJugador { id_jugador, hash: sha256_hex(&token) });
        let suyos = self.tokens.iter().filter(|t| t.id_jugador == id_jugador).count();
        if suyos > MAX_TOKENS_POR_JUGADOR {
            let viejo = self.tokens.iter().position(|t| t.id_jugador == id_jugador).expect("tiene alguno");
            self.tokens.remove(viejo);
        }
        token
    }

    /// El jugador cuyo navegador ha respondido bien al `reto` (con la dirección del túnel por el que llegó).
    pub fn jugador_de_prueba(&self, reto: &str, tunel: &str, respuesta: &str) -> Option<IdJugador> {
        // Se recorren todos sin cortar al encontrarlo (tiempo constante respecto a la posición)
        let mut encontrado = None;
        for t in &self.tokens {
            let Ok(clave) = hex::decode(&t.hash) else { continue };
            if iguales(&prueba(&clave, reto, tunel), respuesta) {
                encontrado = Some(t.id_jugador);
            }
        }
        encontrado
    }

    /// Lo mismo que hace el navegador con su token (para los tests).
    #[cfg(test)]
    pub fn responder(token: &str, reto: &str, tunel: &str) -> String {
        prueba(&Sha256::digest(token.as_bytes()), reto, tunel)
    }

    #[cfg(test)]
    fn jugador_de_token(&self, token: &str) -> Option<IdJugador> {
        self.jugador_de_prueba("r", "t", &SalaGuardada::responder(token, "r", "t"))
    }

    /// Olvida a un jugador: la próxima vez tendrá que poner la contraseña (si la hay).
    pub fn revocar(&mut self, id_jugador: IdJugador) {
        self.tokens.retain(|t| t.id_jugador != id_jugador);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tablero_general::almacen::pruebas::dir_temporal;

    #[test]
    fn se_crea_una_vez_y_se_vuelve_a_leer_igual() {
        let dir = dir_temporal("sala_crear");
        fs::create_dir_all(&dir).unwrap();
        let a = SalaGuardada::leer_o_crear(&dir).unwrap();
        let b = SalaGuardada::leer_o_crear(&dir).unwrap();
        assert_eq!((a.id.clone(), a.secreto.clone()), (b.id, b.secreto));
        assert!(a.id.len() >= 22 && a.secreto.len() >= 22, "128 bits en base64");
        assert!(a.coincide(&a.id, &a.secreto));
        assert!(!a.coincide(&a.id, "otro"));
        assert!(!a.coincide("otra", &a.secreto));

        // Un archivo roto da una sala nueva (el enlace viejo deja de valer)
        fs::write(dir.join(ARCHIVO), "{ roto").unwrap();
        let c = SalaGuardada::leer_o_crear(&dir).unwrap();
        assert_ne!(c.id, a.id);
    }

    #[test]
    fn la_contrasena_es_opcional_y_se_guarda_como_hash() {
        let mut s = SalaGuardada::nueva();
        assert!(!s.pide_contrasena());
        assert!(s.comprobar_contrasena(None) && s.comprobar_contrasena(Some("lo que sea")));

        s.poner_contrasena(Some("dragón 42")).unwrap();
        assert!(s.pide_contrasena());
        let hash = s.hash_contrasena.clone().unwrap();
        assert!(hash.starts_with("$argon2id$") && !hash.contains("dragón"));
        assert!(s.comprobar_contrasena(Some("dragón 42")));
        assert!(!s.comprobar_contrasena(Some("dragon 42")));
        assert!(!s.comprobar_contrasena(None));

        s.poner_contrasena(Some("")).unwrap();
        assert!(!s.pide_contrasena(), "vacía = quitarla");
        assert!(s.poner_contrasena(Some(&"x".repeat(MAX_CONTRASENA + 1))).is_err());
    }

    #[test]
    fn un_token_recuerda_a_su_jugador_hasta_que_se_revoca_o_se_regenera_el_enlace() {
        let mut s = SalaGuardada::nueva();
        let t1 = s.nuevo_token(1);
        let t2 = s.nuevo_token(2);
        assert_eq!(s.jugador_de_token(&t1), Some(1));
        assert_eq!(s.jugador_de_token(&t2), Some(2));
        assert_eq!(s.jugador_de_token("inventado"), None);
        assert!(s.tokens.iter().all(|t| t.hash != t1 && t.hash != t2), "solo se guarda el hash");

        s.revocar(1);
        assert_eq!(s.jugador_de_token(&t1), None);
        assert_eq!(s.jugador_de_token(&t2), Some(2));

        let (id, secreto) = (s.id.clone(), s.secreto.clone());
        s.poner_contrasena(Some("clave")).unwrap();
        s.regenerar();
        assert!(!s.coincide(&id, &secreto), "el enlace viejo deja de valer");
        assert_eq!(s.jugador_de_token(&t2), None, "todos repiten la contraseña");
        assert!(s.pide_contrasena(), "la contraseña se conserva");
    }

    #[test]
    fn la_respuesta_al_reto_solo_vale_para_ese_reto_y_ese_tunel() {
        let mut s = SalaGuardada::nueva();
        let token = s.nuevo_token(4);
        let buena = SalaGuardada::responder(&token, "reto1", "a.trycloudflare.com");
        assert_eq!(s.jugador_de_prueba("reto1", "a.trycloudflare.com", &buena), Some(4));
        assert_eq!(s.jugador_de_prueba("reto2", "a.trycloudflare.com", &buena), None, "otro reto");
        assert_eq!(s.jugador_de_prueba("reto1", "falso.trycloudflare.com", &buena), None, "otro servidor");
        assert_eq!(s.jugador_de_prueba("reto1", "a.trycloudflare.com", "basura"), None);
        // Vector fijo, el mismo que comprueba el cliente web (tests/conexion.test.ts)
        assert_eq!(
            prueba(&Sha256::digest(b"token"), "reto", "x.trycloudflare.com"),
            "193db0e84f792dd66761cc1dc113b7ad299da23603ddaedf9ebb23e7022db4f8"
        );
    }

    #[test]
    fn cada_jugador_tiene_un_numero_limitado_de_dispositivos_recordados() {
        let mut s = SalaGuardada::nueva();
        let primero = s.nuevo_token(1);
        for _ in 0..MAX_TOKENS_POR_JUGADOR {
            s.nuevo_token(1);
        }
        assert_eq!(s.tokens.len(), MAX_TOKENS_POR_JUGADOR);
        assert_eq!(s.jugador_de_token(&primero), None, "se olvida el más viejo");
    }

    #[test]
    fn se_guarda_y_se_lee_con_la_contrasena_y_los_tokens() {
        let dir = dir_temporal("sala_guardar");
        fs::create_dir_all(&dir).unwrap();
        let mut s = SalaGuardada::nueva();
        s.poner_contrasena(Some("abc")).unwrap();
        let t = s.nuevo_token(3);
        s.guardar(&dir).unwrap();
        let leida = SalaGuardada::leer_o_crear(&dir).unwrap();
        assert!(leida.comprobar_contrasena(Some("abc")));
        assert_eq!(leida.jugador_de_token(&t), Some(3));
        assert!(!fs::read_to_string(dir.join(ARCHIVO)).unwrap().contains(&t));
    }
}
