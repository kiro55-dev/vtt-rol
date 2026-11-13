// src-tauri/src/red/trozos.rs
//
// Los mensajes del protocolo (JSON) por un data channel de WebRTC: uno puede ocupar hasta 512 KiB (la `Vista`
// entera) y un mensaje de data channel debe quedarse en 16 KiB para ir bien en cualquier navegador. Así que cada
// mensaje se parte en trozos binarios: 1 byte (`SIGUE` o `ULTIMO`) + hasta `TROZO - 1` bytes del texto. El canal
// `juego` es fiable y ordenado, así que basta con ir juntando hasta el último. Mismo formato en
// `src/lib/red/trozos.ts`.

/// Tamaño máximo de cada trozo (cabecera incluida).
pub const TROZO: usize = 16 * 1024;
pub const SIGUE: u8 = 0;
pub const ULTIMO: u8 = 1;

/// Parte un mensaje en trozos. Uno vacío da un solo trozo con solo la cabecera.
pub fn trocear(texto: &str) -> Vec<Vec<u8>> {
    let datos = texto.as_bytes();
    if datos.is_empty() {
        return vec![vec![ULTIMO]];
    }
    let partes: Vec<&[u8]> = datos.chunks(TROZO - 1).collect();
    let n = partes.len();
    partes
        .into_iter()
        .enumerate()
        .map(|(i, p)| {
            let mut t = Vec::with_capacity(p.len() + 1);
            t.push(if i + 1 == n { ULTIMO } else { SIGUE });
            t.extend_from_slice(p);
            t
        })
        .collect()
}

/// Junta trozos hasta tener un mensaje entero.
pub struct Recomponer {
    datos: Vec<u8>,
    max: usize,
}

impl Recomponer {
    /// `max`: bytes que puede ocupar un mensaje (pasarse es un error: se corta la conexión).
    pub fn nuevo(max: usize) -> Self {
        Recomponer { datos: Vec::new(), max }
    }

    /// Un trozo más. `Ok(Some(texto))` cuando llega el último; `Err` si el trozo no vale, el mensaje se pasa del
    /// tope o no es UTF-8.
    pub fn trozo(&mut self, t: &[u8]) -> Result<Option<String>, ()> {
        let (&cabecera, datos) = t.split_first().ok_or(())?;
        if cabecera != SIGUE && cabecera != ULTIMO {
            return Err(());
        }
        if self.datos.len() + datos.len() > self.max {
            return Err(());
        }
        self.datos.extend_from_slice(datos);
        if cabecera == SIGUE {
            return Ok(None);
        }
        let entero = std::mem::take(&mut self.datos);
        String::from_utf8(entero).map(Some).map_err(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_mensaje_largo_va_en_trozos_y_se_recompone() {
        let texto: String = (0..50_000).map(|i| char::from(b'a' + (i % 26) as u8)).collect::<String>() + "ñ€";
        let trozos = trocear(&texto);
        assert_eq!(trozos.len(), 4);
        assert!(trozos.iter().all(|t| t.len() <= TROZO));
        assert_eq!(trozos.iter().filter(|t| t[0] == ULTIMO).count(), 1);
        let mut r = Recomponer::nuevo(1024 * 1024);
        let mut fuera = None;
        for t in &trozos {
            if let Some(s) = r.trozo(t).unwrap() {
                fuera = Some(s);
            }
        }
        assert_eq!(fuera.unwrap(), texto, "aunque un carácter quede partido entre dos trozos");
        // Y se puede seguir con otro
        assert_eq!(r.trozo(&trocear("hola")[0]).unwrap().as_deref(), Some("hola"));
        assert_eq!(r.trozo(&trocear("")[0]).unwrap().as_deref(), Some(""));
    }

    #[test]
    fn un_trozo_malo_o_un_mensaje_demasiado_grande_es_un_error() {
        let mut r = Recomponer::nuevo(10);
        assert!(r.trozo(&[]).is_err(), "sin cabecera");
        assert!(r.trozo(&[7, b'x']).is_err(), "cabecera desconocida");
        let mut largo = vec![SIGUE];
        largo.extend([0u8; 12]);
        assert!(Recomponer::nuevo(10).trozo(&largo).is_err(), "se pasa del tope");
        assert!(Recomponer::nuevo(10).trozo(&[ULTIMO, 0xff, 0xfe]).is_err(), "no es UTF-8");
    }
}
