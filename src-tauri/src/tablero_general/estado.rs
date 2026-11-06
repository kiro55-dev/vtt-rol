// src-tauri/src/tablero_general/estado.rs
//
// El estado de la partida y los cambios (deltas) que lo modifican. No depende de Tauri:
// solo de los modelos y del `Almacen`, así se puede probar con `cargo test`.
//
// Garantías de persistencia:
//  * Cada delta lleva un número de secuencia (`seq`) creciente y sin saltos.
//  * El snapshot guarda `ultimo_seq`: al cargar solo se reproducen los deltas con
//    `seq > ultimo_seq`. Así, aunque la app se cierre justo entre escribir el snapshot y
//    vaciar el log, ningún delta se aplica dos veces.
//  * El orden en disco = el orden de aplicación, porque `registrar` escribe mientras quien
//    llama tiene cogido el lock del estado.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::tablero_general::almacen::Almacen;
use crate::tablero_general::modelos::*;

// ========================================================
// 1. DELTAS (eventos de la partida)
// ========================================================
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum DeltaPartida {
    TiradaRealizada(Tiradas),
    VidaModificada { id_instancia: u16, delta: i16 },
    ZonaCreada(Zona),
    ZonaEliminada(Zona),
    ZonaModificada(Zona),
    MapaCreado(Zona, Mapa),
    MapaModificado(Mapa),
    MapaBorrado(Mapa),
    EntidadCreada(Entidad),
    EntidadBorrada(Entidad),
    EntidadModificada(Entidad)
    /* 
    InstanciaCreada(),
    InstanciaModificada(),
    InstanciaBorrada()
    */
}

/// Línea de deltas.jsonl al LEER.
#[derive(Deserialize)]
struct LineaDelta {
    seq: u64,
    delta: DeltaPartida,
}

/// Línea de deltas.jsonl al ESCRIBIR (mismo formato, pero sin clonar el delta).
#[derive(Serialize)]
struct LineaDeltaRef<'a> {
    seq: u64,
    delta: &'a DeltaPartida,
}

// ========================================================
// 2. ESTADO GENERAL
// ========================================================
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct EstadoPartida {
    /// Número del último delta ya incluido en este estado.
    #[serde(default)]
    pub ultimo_seq: u64,
    #[serde(default)]
    pub zonas: Vec<Zona>,
    #[serde(default)]
    pub mapas: Vec<Mapa>,
    #[serde(default)]
    pub banco_entidades: Vec<Entidad>,
    #[serde(default)]
    pub instancias_activas: Vec<Instancia>,
    #[serde(default)]
    pub historial_tiradas: Vec<Tiradas>,
}

/// Qué pasó al cargar la partida (para mostrarlo en el log y en los tests).
#[derive(Debug, Default)]
pub struct InformeCarga {
    pub deltas_aplicados: usize,
    pub deltas_ya_incluidos: usize,
    pub lineas_invalidas: usize,
    pub partida_corrupta_respaldada_en: Option<PathBuf>,
}

impl InformeCarga {
    /// Si hay deltas por consolidar o líneas basura conviene reescribir el snapshot al arrancar.
    pub fn conviene_compactar(&self) -> bool {
        self.deltas_aplicados > 0 || self.deltas_ya_incluidos > 0 || self.lineas_invalidas > 0
    }
}

impl EstadoPartida {
    // Aplica un delta modificando el estado en memoria (lógica sin cambios respecto a tu versión)
    pub fn aplicar_delta(&mut self, delta: &DeltaPartida) {
        match delta {
            DeltaPartida::TiradaRealizada(tirada) => {
                self.historial_tiradas.insert(0, tirada.clone());
                if self.historial_tiradas.len() > 20 {
                    self.historial_tiradas.pop();
                }
            }
            DeltaPartida::VidaModificada { id_instancia, delta } => {
                if let Some(inst) = self
                    .instancias_activas
                    .iter_mut()
                    .find(|i| i.id_instancia == *id_instancia)
                {
                    inst.vida_actual += delta;
                }
            }
            DeltaPartida::ZonaCreada(zona) => {
                if let Some(pos) = self.zonas.iter().position(|z| z.id == zona.id) {
                    self.zonas[pos] = zona.clone();
                } else {
                    self.zonas.push(zona.clone());
                }
            }
            DeltaPartida::ZonaEliminada(zona) => {
                // 1. Sacamos los IDs de los mapas que pertenecían a esta zona
                let mapas_eliminados: Vec<u16> = self
                    .mapas
                    .iter()
                    .filter(|m| m.id_zona == zona.id)
                    .map(|m| m.id)
                    .collect();

                // 2. Eliminamos la zona y sus mapas
                self.zonas.retain(|z| z.id != zona.id);
                self.mapas.retain(|m| m.id_zona != zona.id);

                // 3. Procesamos las instancias activas
                self.instancias_activas.retain_mut(|inst| {
                    let estaba_en_mapa_afectado = inst
                        .id_mapa
                        .map_or(false, |id_m| mapas_eliminados.contains(&id_m));
                    let pertenece_a_zona = inst.base.zona_id == zona.id;

                    if estaba_en_mapa_afectado || pertenece_a_zona {
                        if inst.base.es_unico {
                            // ÚNICA: se conserva, sin mapa y desactivada
                            if estaba_en_mapa_afectado {
                                inst.id_mapa = None;
                                inst.coordenadas = Coordenada::NoPuesto;
                            }
                            inst.activo = false;
                            true
                        } else {
                            false
                        }
                    } else {
                        true
                    }
                });

                // 4. En el banco de entidades, conservamos las únicas
                self.banco_entidades
                    .retain(|ent| ent.es_unico || ent.zona_id != zona.id);
            }
            DeltaPartida::ZonaModificada(zona) => {
                if let Some(pos) = self.zonas.iter().position(|z| z.id == zona.id) {
                    self.zonas[pos] = zona.clone();
                }
            }
            DeltaPartida::MapaCreado(zona, mapa) => {
                let mut nuevo_mapa = mapa.clone();
                nuevo_mapa.id_zona = zona.id;

                if let Some(pos) = self.mapas.iter().position(|m| m.id == nuevo_mapa.id) {
                    self.mapas[pos] = nuevo_mapa;
                } else {
                    self.mapas.push(nuevo_mapa);
                }
            }
            DeltaPartida::MapaModificado(mapa) => {
                if let Some(pos) = self.mapas.iter().position(|m| m.id == mapa.id) {
                    self.mapas[pos] = mapa.clone();
                }
            }
            DeltaPartida::MapaBorrado(mapa) => {
                self.mapas.retain(|m| m.id != mapa.id);

                self.instancias_activas.retain_mut(|inst| {
                    if inst.id_mapa == Some(mapa.id) {
                        if inst.base.es_unico {
                            inst.id_mapa = None;
                            inst.activo = false;
                            inst.coordenadas = Coordenada::NoPuesto;
                            true
                        } else {
                            false
                        }
                    } else {
                        true
                    }
                });
            }
            DeltaPartida::EntidadCreada(entidad) => {
                if let Some(pos) = self.banco_entidades.iter().position(|e| e.id == entidad.id) {
                    self.banco_entidades[pos] = entidad.clone();
                } else {
                    self.banco_entidades.push(entidad.clone());
                }
            }
            DeltaPartida::EntidadModificada(entidad) => {
                // 1. Actualizamos la plantilla base en el banco de entidades
                if let Some(pos) = self.banco_entidades.iter().position(|e| e.id == entidad.id) {
                    self.banco_entidades[pos] = entidad.clone();
                }

                // 2. Sincronizamos la información base en todas las instancias activas de esta entidad
                for inst in self.instancias_activas.iter_mut() {
                    if inst.base.id == entidad.id {
                        inst.base = entidad.clone();
                    }
                }
            }
            DeltaPartida::EntidadBorrada(entidad) => {
                // 1. La eliminamos del banco de entidades
                self.banco_entidades.retain(|e| e.id != entidad.id);

                // 2. Se eliminan todas las instancias asociadas en el tablero/activas
                self.instancias_activas.retain(|inst| inst.base.id != entidad.id);
            }
        }
    }

    /// Aplica el delta en memoria Y lo añade al log en disco, con el siguiente `seq`.
    /// Llamar siempre con el lock del estado cogido.
    pub fn registrar(&mut self, almacen: &Almacen, delta: DeltaPartida) -> Result<(), String> {
        let seq = self.ultimo_seq + 1;
        self.aplicar_delta(&delta);
        self.ultimo_seq = seq;

        // El cambio ya está en memoria aunque el disco falle; el error se devuelve para que se vea.
        let linea = serde_json::to_string(&LineaDeltaRef { seq, delta: &delta })
            .map_err(|e| format!("no se pudo serializar el delta {seq}: {e}"))?;
        almacen
            .anadir_delta(&linea)
            .map_err(|e| format!("no se pudo guardar el delta {seq}: {e}"))
    }
}

// ========================================================
// 3. CARGA Y COMPACTACIÓN
// ========================================================

/// Lee `partida.json` y reproduce encima los deltas pendientes.
/// Falla (y la app no debe arrancar) solo si el disco no se puede LEER: seguir adelante
/// con un estado vacío y guardarlo después destruiría una partida que quizá está bien.
/// Un JSON ilegible se aparta a `partida.corrupta-*.json` y se continúa.
pub fn cargar(almacen: &Almacen) -> Result<(EstadoPartida, InformeCarga), String> {
    let mut informe = InformeCarga::default();

    let mut estado = match almacen
        .leer_partida()
        .map_err(|e| format!("no se pudo leer partida.json: {e}"))?
    {
        None => EstadoPartida::default(),
        Some(json) => match serde_json::from_str::<EstadoPartida>(&json) {
            Ok(e) => e,
            Err(err) => {
                eprintln!("[partida] partida.json ilegible ({err}); se aparta y se empieza vacío");
                informe.partida_corrupta_respaldada_en = almacen.respaldar_partida_corrupta();
                EstadoPartida::default()
            }
        },
    };

    let lineas = almacen
        .leer_deltas()
        .map_err(|e| format!("no se pudo leer deltas.jsonl: {e}"))?;

    for linea in lineas.iter().map(|l| l.trim()).filter(|l| !l.is_empty()) {
        if let Ok(l) = serde_json::from_str::<LineaDelta>(linea) {
            if l.seq <= estado.ultimo_seq {
                informe.deltas_ya_incluidos += 1; // ya estaba dentro del snapshot
                continue;
            }
            estado.aplicar_delta(&l.delta);
            estado.ultimo_seq = l.seq;
            informe.deltas_aplicados += 1;
        } else if let Ok(d) = serde_json::from_str::<DeltaPartida>(linea) {
            // Formato antiguo (sin seq), solo aparece una vez al migrar datos viejos.
            estado.aplicar_delta(&d);
            estado.ultimo_seq += 1;
            informe.deltas_aplicados += 1;
        } else {
            // Típicamente la última línea, cortada por un cierre brusco.
            informe.lineas_invalidas += 1;
            eprintln!("[partida] línea de deltas ignorada: {linea:.80}");
        }
    }

    Ok((estado, informe))
}

/// Escribe el snapshot de forma atómica y vacía el log.
/// Con el lock del estado cogido. Si se corta tras el snapshot y antes de vaciar el log,
/// `cargar` descarta esos deltas por su `seq`.
pub fn compactar(almacen: &Almacen, estado: &EstadoPartida) -> Result<(), String> {
    let json = serde_json::to_string_pretty(estado)
        .map_err(|e| format!("no se pudo serializar la partida: {e}"))?;
    almacen
        .escribir_partida(&json)
        .map_err(|e| format!("no se pudo escribir partida.json: {e}"))?;
    almacen
        .vaciar_deltas()
        .map_err(|e| format!("no se pudo vaciar deltas.jsonl: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tablero_general::almacen::pruebas::dir_temporal;
    use std::fs::{self, OpenOptions};
    use std::io::Write;

    fn zona(id: u16) -> Zona {
        Zona { id, nombre: format!("Zona {id}"), orden: 0 }
    }

    fn mapa(id: u16, id_zona: u16) -> Mapa {
        Mapa {
            id,
            id_zona,
            nombre: id.to_string(),
            url_imagen_fondo: Some("imagenes/x.png".into()),
            ancho: 1000,
            alto: 1000,
            rejilla: 50,
            color_opacidad: vec![255, 255, 255, 76],
            grosor: 1,
            orden: 0,
        }
    }

    fn instancia(id: u16, vida: i16) -> Instancia {
        Instancia {
            id_instancia: id,
            id_mapa: None,
            base: Entidad {
                id: 1,
                nombre: "Orco".into(),
                tipo: TipoEntidad::Enemigo,
                vida_max: 20,
                vida_es_publica: true,
                armadura: 1,
                armadura_es_publica: false,
                velocidad: 6,
                rango_arma: 1,
                cuerpo: Cuerpo::Pequenio,
                zona_id: 1,
                foto_url: None,
                archivos_adjuntos: vec![],
                es_unico: false,
            },
            vida_actual: vida,
            coordenadas: Coordenada::NoPuesto,
            activo: true,
        }
    }

    #[test]
    fn registrar_y_recargar_reproduce_el_estado() {
        let dir = dir_temporal("recarga");
        {
            let a = Almacen::abrir(&dir).unwrap();
            let mut e = EstadoPartida::default();
            e.registrar(&a, DeltaPartida::ZonaCreada(zona(1))).unwrap();
            e.registrar(&a, DeltaPartida::MapaCreado(zona(1), mapa(1, 1))).unwrap();
            e.registrar(&a, DeltaPartida::MapaModificado(Mapa { rejilla: 70, ..mapa(1, 1) })).unwrap();
        }
        let a = Almacen::abrir(&dir).unwrap();
        let (e, informe) = cargar(&a).unwrap();
        assert_eq!(informe.deltas_aplicados, 3);
        assert_eq!(e.ultimo_seq, 3);
        assert_eq!(e.zonas.len(), 1);
        assert_eq!(e.mapas[0].rejilla, 70);
    }

    #[test]
    fn un_corte_entre_snapshot_y_vaciado_no_duplica_deltas() {
        let a = Almacen::abrir(dir_temporal("corte")).unwrap();
        let mut e = EstadoPartida::default();
        e.instancias_activas.push(instancia(1, 10));
        e.registrar(&a, DeltaPartida::VidaModificada { id_instancia: 1, delta: -3 }).unwrap();
        assert_eq!(e.instancias_activas[0].vida_actual, 7);

        // Simula el corte: el snapshot se escribe, pero deltas.jsonl NO llega a vaciarse.
        a.escribir_partida(&serde_json::to_string(&e).unwrap()).unwrap();

        let (e2, informe) = cargar(&a).unwrap();
        assert_eq!(e2.instancias_activas[0].vida_actual, 7, "la vida no debe bajar dos veces");
        assert_eq!(informe.deltas_ya_incluidos, 1);
        assert_eq!(informe.deltas_aplicados, 0);
    }

    #[test]
    fn un_snapshot_ilegible_se_aparta_y_no_se_pierde() {
        let a = Almacen::abrir(dir_temporal("corrupto")).unwrap();
        a.escribir_partida("{ esto no es json").unwrap();

        let (e, informe) = cargar(&a).unwrap();
        assert!(e.zonas.is_empty());
        let respaldo = informe.partida_corrupta_respaldada_en.expect("debe haberse respaldado");
        assert_eq!(fs::read_to_string(respaldo).unwrap(), "{ esto no es json");
        assert!(a.leer_partida().unwrap().is_none(), "partida.json ya no debe estar en medio");
    }

    #[test]
    fn una_linea_cortada_se_ignora_y_la_siguiente_no_se_pega_a_ella() {
        let dir = dir_temporal("cortada");
        {
            let a = Almacen::abrir(&dir).unwrap();
            let mut e = EstadoPartida::default();
            e.registrar(&a, DeltaPartida::ZonaCreada(zona(1))).unwrap();
            e.registrar(&a, DeltaPartida::ZonaCreada(zona(2))).unwrap();
        }
        // Cierre brusco: media línea sin salto de línea final.
        let mut f = OpenOptions::new().append(true).open(dir.join("deltas.jsonl")).unwrap();
        f.write_all(br#"{"seq":3,"delta":{"ZonaCre"#).unwrap();
        drop(f);

        // Arranque siguiente: se ignora la línea rota y se sigue escribiendo bien.
        let a = Almacen::abrir(&dir).unwrap();
        let (mut e, informe) = cargar(&a).unwrap();
        assert_eq!((informe.deltas_aplicados, informe.lineas_invalidas), (2, 1));
        e.registrar(&a, DeltaPartida::ZonaCreada(zona(3))).unwrap();

        let a = Almacen::abrir(&dir).unwrap();
        let (e, informe) = cargar(&a).unwrap();
        assert_eq!(e.zonas.len(), 3, "el delta nuevo no debe haberse pegado a la línea rota");
        assert_eq!(informe.lineas_invalidas, 1);
    }

    #[test]
    fn acepta_el_formato_antiguo_sin_seq() {
        let a = Almacen::abrir(dir_temporal("antiguo")).unwrap();
        a.anadir_delta(r#"{"ZonaCreada":{"id":1,"nombre":"Vieja","orden":0}}"#).unwrap();
        let (e, informe) = cargar(&a).unwrap();
        assert_eq!(e.zonas[0].nombre, "Vieja");
        assert_eq!(e.ultimo_seq, 1);
        assert!(informe.conviene_compactar());
    }

    #[test]
    fn compactar_vacia_el_log_y_la_secuencia_continua() {
        let dir = dir_temporal("compactar");
        let a = Almacen::abrir(&dir).unwrap();
        let mut e = EstadoPartida::default();
        e.registrar(&a, DeltaPartida::ZonaCreada(zona(1))).unwrap();
        e.registrar(&a, DeltaPartida::ZonaCreada(zona(2))).unwrap();
        compactar(&a, &e).unwrap();
        assert!(a.leer_deltas().unwrap().is_empty());

        // Se puede seguir escribiendo en el mismo Almacen tras vaciar (el handle se reabre).
        e.registrar(&a, DeltaPartida::ZonaCreada(zona(3))).unwrap();
        let lineas = a.leer_deltas().unwrap();
        assert_eq!(lineas.len(), 1);
        assert!(lineas[0].starts_with(r#"{"seq":3,"#), "la secuencia no se reinicia: {}", lineas[0]);

        let (e2, informe) = cargar(&Almacen::abrir(&dir).unwrap()).unwrap();
        assert_eq!((e2.zonas.len(), e2.ultimo_seq, informe.deltas_aplicados), (3, 3, 1));
    }
}
