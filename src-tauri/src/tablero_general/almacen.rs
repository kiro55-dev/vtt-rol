// src-tauri/src/tablero_general/almacen.rs
//
// Acceso a disco de la partida. Todo vive en UN directorio (el de datos de la app,
// p. ej. %APPDATA%\<identifier> en Windows), nunca en el directorio de trabajo:
//
//   partida.json           snapshot completo del estado
//   deltas.jsonl           cambios posteriores al snapshot, una línea por cambio
//   historial_personal.txt últimos comandos de dados escritos por este usuario
//   imagenes/              fondos de mapa importados
//
// Este módulo no sabe nada de Tauri ni de los modelos: solo mueve texto y archivos.
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, RwLock};

const PARTIDA: &str = "partida.json";
const PARTIDA_TMP: &str = "partida.json.tmp";
const DELTAS: &str = "deltas.jsonl";
const HISTORIAL: &str = "historial_personal.txt";
const IMAGENES: &str = "imagenes";
const MAX_HISTORIAL: usize = 100;

/// Un Mutex envenenado (otro hilo hizo panic con él cogido) no debe tumbar toda la app.
pub fn bloquear<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct Almacen {
    /// Carpeta de la partida abierta. Cambia al alternar de partida (`cambiar_dir`).
    dir: RwLock<PathBuf>,
    /// Archivo de deltas abierto en modo append. Se abre la primera vez que hace falta.
    deltas: Mutex<Option<File>>,
    /// Serializa lectura+escritura del historial personal.
    historial: Mutex<()>,
}

impl Almacen {
    pub fn abrir(dir: impl Into<PathBuf>) -> io::Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(dir.join(IMAGENES))?;
        Ok(Self {
            dir: RwLock::new(dir),
            deltas: Mutex::new(None),
            historial: Mutex::new(()),
        })
    }

    /// Copia de la ruta actual: no se devuelve una referencia porque puede cambiar al alternar de partida.
    pub fn dir(&self) -> PathBuf {
        self.dir.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Pasa a trabajar con la carpeta de otra partida. Suelta el archivo de deltas abierto (si no,
    /// el siguiente cambio se escribiría en la partida anterior). Quien llama debe tener cogido el
    /// lock del estado, y haber guardado antes la partida que se deja.
    pub fn cambiar_dir(&self, nuevo: impl Into<PathBuf>) -> io::Result<()> {
        let nuevo = nuevo.into();
        fs::create_dir_all(nuevo.join(IMAGENES))?;
        let mut deltas = bloquear(&self.deltas);
        *deltas = None;
        *self.dir.write().unwrap_or_else(|e| e.into_inner()) = nuevo;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Snapshot
    // ------------------------------------------------------------------

    /// `Ok(None)` si todavía no existe (primera ejecución). Cualquier otro error se propaga.
    pub fn leer_partida(&self) -> io::Result<Option<String>> {
        match fs::read(self.dir().join(PARTIDA)) {
            Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Escritura atómica: archivo temporal + fsync + rename. Si se corta la luz a mitad,
    /// `partida.json` sigue siendo la versión anterior completa.
    pub fn escribir_partida(&self, json: &str) -> io::Result<()> {
        let tmp = self.dir().join(PARTIDA_TMP);
        {
            let mut f = File::create(&tmp)?;
            f.write_all(json.as_bytes())?;
            f.sync_all()?;
        }
        fs::rename(&tmp, self.dir().join(PARTIDA))
    }

    /// Aparta un `partida.json` ilegible para no sobrescribirlo en la siguiente compactación.
    pub fn respaldar_partida_corrupta(&self) -> Option<PathBuf> {
        let marca = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let destino = self.dir().join(format!("partida.corrupta-{marca}.json"));
        fs::rename(self.dir().join(PARTIDA), &destino).ok().map(|_| destino)
    }

    // ------------------------------------------------------------------
    // Deltas
    // ------------------------------------------------------------------

    pub fn leer_deltas(&self) -> io::Result<Vec<String>> {
        match fs::read(self.dir().join(DELTAS)) {
            Ok(bytes) => Ok(String::from_utf8_lossy(&bytes)
                .lines()
                .map(|l| l.to_string())
                .collect()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    /// Añade UNA línea al final. Quien llama debe invocarlo con el lock del estado cogido,
    /// así el orden en disco es siempre el orden en que se aplicaron los cambios.
    pub fn anadir_delta(&self, linea: &str) -> io::Result<()> {
        let mut guardia = bloquear(&self.deltas);

        if guardia.is_none() {
            let ruta = self.dir().join(DELTAS);
            let mut f = OpenOptions::new().create(true).append(true).open(&ruta)?;

            // Si la app se cerró a mitad de una línea, el archivo no acaba en '\n':
            // lo cerramos para que la siguiente línea no quede pegada a la rota.
            let len = f.metadata()?.len();
            if len > 0 {
                let mut lector = File::open(&ruta)?;
                lector.seek(SeekFrom::Start(len - 1))?;
                let mut ultimo = [0u8; 1];
                lector.read_exact(&mut ultimo)?;
                if ultimo[0] != b'\n' {
                    f.write_all(b"\n")?;
                }
            }
            *guardia = Some(f);
        }

        let mut buf = String::with_capacity(linea.len() + 1);
        buf.push_str(linea);
        buf.push('\n');

        // Un solo write_all por línea; si falla, soltamos el handle para reabrirlo la próxima vez.
        let resultado = guardia.as_mut().map(|f| f.write_all(buf.as_bytes()));
        match resultado {
            Some(Ok(())) => Ok(()),
            Some(Err(e)) => {
                *guardia = None;
                Err(e)
            }
            None => Ok(()),
        }
    }

    /// Vacía deltas.jsonl. Se hace cerrando el handle (en Windows un archivo abierto en modo
    /// append no se puede truncar) y recreando el archivo.
    pub fn vaciar_deltas(&self) -> io::Result<()> {
        let mut guardia = bloquear(&self.deltas);
        *guardia = None;
        File::create(self.dir().join(DELTAS)).map(|_| ())
    }

    // ------------------------------------------------------------------
    // Historial personal de comandos
    // ------------------------------------------------------------------

    fn leer_historial_interno(&self) -> Vec<String> {
        fs::read(self.dir().join(HISTORIAL))
            .map(|b| {
                String::from_utf8_lossy(&b)
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn leer_historial(&self) -> Vec<String> {
        let _g = bloquear(&self.historial);
        self.leer_historial_interno()
    }

    pub fn anadir_historial(&self, comando: &str) {
        let cmd = comando.replace(['\n', '\r'], " ").trim().to_string();
        if cmd.is_empty() {
            return;
        }

        let _g = bloquear(&self.historial);
        let mut lineas = self.leer_historial_interno();

        if lineas.last() != Some(&cmd) {
            lineas.push(cmd);
        }
        if lineas.len() > MAX_HISTORIAL {
            let sobrante = lineas.len() - MAX_HISTORIAL;
            lineas.drain(0..sobrante);
        }

        let mut salida = lineas.join("\n");
        salida.push('\n');
        if let Err(e) = fs::write(self.dir().join(HISTORIAL), salida) {
            eprintln!("[almacen] no se pudo guardar el historial personal: {e}");
        }
    }

    // ------------------------------------------------------------------
    // Migración puntual desde el sistema antiguo (archivos junto al ejecutable / en src-tauri)
    // ------------------------------------------------------------------

    /// Copia los archivos antiguos al directorio de datos, SOLO si aquí todavía no hay partida.
    /// No borra los originales. Devuelve los nombres copiados.
    pub fn migrar_desde(&self, origen: &Path) -> Vec<&'static str> {
        let ya_hay_datos = self.dir().join(PARTIDA).exists() || self.dir().join(DELTAS).exists();
        if ya_hay_datos || origen == self.dir() {
            return Vec::new();
        }

        let mut copiados = Vec::new();
        for nombre in [PARTIDA, DELTAS, HISTORIAL] {
            let desde = origen.join(nombre);
            if desde.is_file() && fs::copy(&desde, self.dir().join(nombre)).is_ok() {
                copiados.push(nombre);
            }
        }
        copiados
    }
}

#[cfg(test)]
pub mod pruebas {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static CONTADOR: AtomicU32 = AtomicU32::new(0);

    /// Directorio temporal único para cada prueba.
    pub fn dir_temporal(nombre: &str) -> PathBuf {
        let n = CONTADOR.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("vtt_test_{}_{}_{}", nombre, std::process::id(), n));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }
}

#[cfg(test)]
mod tests {
    use super::pruebas::dir_temporal;
    use super::*;

    #[test]
    fn el_historial_no_repite_el_ultimo_y_se_limita_a_100() {
        let a = Almacen::abrir(dir_temporal("historial")).unwrap();
        a.anadir_historial("1d20");
        a.anadir_historial("1d20");
        assert_eq!(a.leer_historial(), vec!["1d20"]);

        for i in 0..150 {
            a.anadir_historial(&format!("{i}d6"));
        }
        let h = a.leer_historial();
        assert_eq!(h.len(), MAX_HISTORIAL);
        assert_eq!(h.last().unwrap(), "149d6");
    }

    #[test]
    fn migrar_copia_una_sola_vez_y_no_pisa_datos_nuevos() {
        let origen = dir_temporal("origen");
        fs::create_dir_all(&origen).unwrap();
        fs::write(origen.join("partida.json"), "{}").unwrap();
        fs::write(origen.join("deltas.jsonl"), "").unwrap();

        let a = Almacen::abrir(dir_temporal("destino")).unwrap();
        assert_eq!(a.migrar_desde(&origen), vec!["partida.json", "deltas.jsonl"]);
        // Segunda vez: ya hay datos, no se toca nada.
        fs::write(a.dir().join("partida.json"), "{\"nuevo\":true}").unwrap();
        assert!(a.migrar_desde(&origen).is_empty());
        assert_eq!(fs::read_to_string(a.dir().join("partida.json")).unwrap(), "{\"nuevo\":true}");
    }
}
