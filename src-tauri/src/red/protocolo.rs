// src-tauri/src/red/protocolo.rs
//
// Mensajes entre el navegador del jugador y el host, en JSON por el WebSocket. Se reflejan a mano en
// cliente-web/src/protocolo.ts: si cambias algo aquí, cámbialo allí y sube `VERSION_PROTOCOLO` (el cliente
// que no coincide pide recargar la página en vez de malinterpretar los mensajes).
//
// Saludo:
//     host:  Reto { reto }           (nada más abrirse el WebSocket)
//   cliente: Hola { sala, secreto, prueba? }   (prueba = HMAC de su clave sobre reto|túnel, si ya entró)
//     host:  Rechazado        (enlace que no vale, versión distinta...; se cierra)
//            Bienvenido       (prueba válida: se le recuerda y entra directamente)
//            Identificate     (hay que dar nombre y, si la hay, contraseña)
//   cliente: Unirse { nombre, contrasena? }
//     host:  Bienvenido { token }   (el navegador guarda el token: la próxima vez no se pregunta nada)
//            Identificate { error } (contraseña mal; tras varios intentos, Rechazado)
// Después: Vista (entera) y Cambios (solo lo que cambia), Aviso (ping, recorrido) y SalaCerrada.
use serde::{Deserialize, Serialize};

use crate::red::vista::{Cambios, VistaJugador};
use crate::tablero_general::modelos::Jugador;
use crate::tablero_general::tablero::AvisoTablero;

/// Súbelo cuando cambie cualquier mensaje.
pub const VERSION_PROTOCOLO: u32 = 2;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "tipo")]
pub enum MensajeCliente {
    Hola {
        version: u32,
        sala: String,
        secreto: String,
        /// Si el navegador ya entró antes: HMAC-SHA256(clave, "<reto>|<túnel>") (ver sala::prueba).
        #[serde(default)]
        prueba: Option<String>,
    },
    Unirse {
        nombre: String,
        #[serde(default)]
        contrasena: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "tipo")]
pub enum MensajeHost {
    /// Lo primero que envía el host: el reto al que responde un navegador recordado.
    Reto {
        reto: String,
    },
    Rechazado {
        motivo: String,
    },
    Identificate {
        pide_contrasena: bool,
        /// Por qué se vuelve a preguntar (contraseña incorrecta, nombre vacío...).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    Bienvenido {
        yo: Jugador,
        /// Solo al entrar con nombre (y contraseña): el navegador lo guarda para entrar solo la próxima vez.
        /// Es la única vez que viaja: después, el navegador solo demuestra que lo tiene (ver `Hola::prueba`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        token: Option<String>,
        /// Para pedir imágenes (`/imagen?sesion=…`) mientras dure esta conexión.
        sesion: String,
    },
    Vista {
        vista: VistaJugador,
    },
    Cambios {
        cambios: Cambios,
    },
    Aviso {
        aviso: AvisoTablero,
    },
    /// El host cerró la sala (o la aplicación).
    SalaCerrada,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tablero_general::modelos::Casilla;

    #[test]
    fn los_mensajes_tienen_la_forma_que_espera_el_cliente_web() {
        let hola: MensajeCliente =
            serde_json::from_str(r#"{"tipo":"Hola","version":1,"sala":"s","secreto":"k"}"#).unwrap();
        assert_eq!(hola, MensajeCliente::Hola { version: 1, sala: "s".into(), secreto: "k".into(), prueba: None });
        let unirse: MensajeCliente = serde_json::from_str(r#"{"tipo":"Unirse","nombre":"Ana"}"#).unwrap();
        assert_eq!(unirse, MensajeCliente::Unirse { nombre: "Ana".into(), contrasena: None });
        assert!(serde_json::from_str::<MensajeCliente>(r#"{"tipo":"Otro"}"#).is_err());

        let json = |m: &MensajeHost| serde_json::to_string(m).unwrap();
        assert_eq!(json(&MensajeHost::SalaCerrada), r#"{"tipo":"SalaCerrada"}"#);
        assert_eq!(
            json(&MensajeHost::Identificate { pide_contrasena: true, error: None }),
            r#"{"tipo":"Identificate","pide_contrasena":true}"#
        );
        let aviso = MensajeHost::Aviso { aviso: AvisoTablero::Ping { id_mapa: 1, casilla: Casilla { x: 2, y: 3 } } };
        assert_eq!(json(&aviso), r#"{"tipo":"Aviso","aviso":{"Ping":{"id_mapa":1,"casilla":{"x":2,"y":3}}}}"#);
        // Un cambio vacío no lleva campos
        assert_eq!(json(&MensajeHost::Cambios { cambios: Cambios::default() }), r#"{"tipo":"Cambios","cambios":{}}"#);
        // Ida y vuelta
        let b = MensajeHost::Bienvenido { yo: Jugador { id: 3, nombre: "Eva".into() }, token: Some("t".into()), sesion: "x".into() };
        assert_eq!(serde_json::from_str::<MensajeHost>(&json(&b)).unwrap(), b);
    }
}
