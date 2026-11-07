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
use crate::tablero_general::tablero;

// ========================================================
// 1. DELTAS (eventos de la partida)
// ========================================================
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum DeltaPartida {
    TiradaRealizada(Tiradas),
    /// Cambia el valor actual (p. ej. la vida) de un parámetro con máximo en una instancia.
    ValorActualModificado { id_instancia: u16, id_definicion: u16, delta: i16 },
    ZonaCreada(Zona),
    ZonaEliminada(Zona),
    ZonaModificada(Zona),
    MapaCreado(Zona, Mapa),
    MapaModificado(Mapa),
    MapaBorrado(Mapa),
    EntidadCreada(Entidad),
    EntidadBorrada(Entidad),
    EntidadModificada(Entidad),
    /// Define un parámetro nuevo (o reemplaza el de ese id). Las fichas lo reciben con su valor por defecto.
    ParametroCreado(DefinicionParametro),
    /// Cambia la configuración de un parámetro existente (mismo id). Las fichas conservan sus valores.
    ParametroModificado(DefinicionParametro),
    /// Quita un parámetro de la partida y, con él, su valor en todas las fichas e instancias.
    ParametroBorrado(u16),
    /// Nuevo orden de los parámetros (ids). Las fichas lo siguen.
    ParametrosReordenados(Vec<u16>)
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
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EstadoPartida {
    /// Número del último delta ya incluido en este estado.
    #[serde(default)]
    pub ultimo_seq: u64,
    /// Parámetros comunes a todas las fichas. Las partidas anteriores a ellos arrancan con los de serie.
    #[serde(default = "parametros_por_defecto")]
    pub definiciones_parametros: Vec<DefinicionParametro>,
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

// A mano y no con derive: una partida nueva también nace con los parámetros de serie.
impl Default for EstadoPartida {
    fn default() -> Self {
        Self {
            ultimo_seq: 0,
            definiciones_parametros: parametros_por_defecto(),
            zonas: Vec::new(),
            mapas: Vec::new(),
            banco_entidades: Vec::new(),
            instancias_activas: Vec::new(),
            historial_tiradas: Vec::new(),
        }
    }
}

/// Qué pasó al cargar la partida (para mostrarlo en el log y en los tests).
#[derive(Debug, Default)]
pub struct InformeCarga {
    pub deltas_aplicados: usize,
    pub deltas_ya_incluidos: usize,
    pub lineas_invalidas: usize,
    pub partida_corrupta_respaldada_en: Option<PathBuf>,
    /// No había `partida.json` (partida recién creada) o era ilegible: se empieza con el estado de serie.
    pub partida_nueva: bool,
}

impl InformeCarga {
    /// Si hay deltas por consolidar, líneas basura o la partida es nueva (así sus parámetros de serie
    /// quedan escritos en su `partida.json` desde el principio) conviene reescribir el snapshot.
    pub fn conviene_compactar(&self) -> bool {
        self.deltas_aplicados > 0 || self.deltas_ya_incluidos > 0 || self.lineas_invalidas > 0 || self.partida_nueva
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
            DeltaPartida::ValorActualModificado { id_instancia, id_definicion, delta } => {
                if let Some(v) = self
                    .instancias_activas
                    .iter_mut()
                    .find(|i| i.id_instancia == *id_instancia)
                    .and_then(|i| i.valores_actuales.iter_mut().find(|v| v.id_definicion == *id_definicion))
                {
                    v.valor = v.valor.saturating_add(*delta);
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

                    // Si la rejilla o el tamaño cambian, una ficha podría quedar fuera: vuelve al borde.
                    let rejilla = tablero::Rejilla::de(mapa);
                    for inst in self.instancias_activas.iter_mut().filter(|i| i.id_mapa == Some(mapa.id)) {
                        if let Coordenada::Casilla(c) = inst.coordenadas {
                            let lados = tablero::tamano_cuerpo(&inst.base.cuerpo);
                            inst.coordenadas = Coordenada::Casilla(tablero::encajar(rejilla, c, lados));
                        }
                    }
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
            DeltaPartida::ParametroCreado(def) | DeltaPartida::ParametroModificado(def) => {
                match self.definiciones_parametros.iter().position(|d| d.id == def.id) {
                    Some(pos) => self.definiciones_parametros[pos] = def.clone(),
                    None => self.definiciones_parametros.push(def.clone()),
                }
                self.ajustar_a_definiciones();
            }
            DeltaPartida::ParametroBorrado(id) => {
                self.definiciones_parametros.retain(|d| d.id != *id);
                self.ajustar_a_definiciones();
            }
            DeltaPartida::ParametrosReordenados(ids) => {
                // Los que no aparezcan en la lista (no debería pasar) se quedan al final, en su orden
                self.definiciones_parametros
                    .sort_by_key(|d| ids.iter().position(|i| *i == d.id).unwrap_or(usize::MAX));
                self.ajustar_a_definiciones();
            }
        }
    }

    /// Deja las fichas e instancias acordes con las definiciones: un parámetro por definición y en su
    /// orden (rellena con el valor por defecto, quita los de definiciones que ya no existen) y solo
    /// lleva valor actual lo que sigue teniendo máximo.
    pub fn ajustar_a_definiciones(&mut self) {
        let defs = self.definiciones_parametros.clone();
        for e in self.banco_entidades.iter_mut() {
            normalizar_parametros(&defs, &mut e.parametros);
        }
        for i in self.instancias_activas.iter_mut() {
            normalizar_parametros(&defs, &mut i.base.parametros);
            i.valores_actuales.retain(|v| defs.iter().any(|d| d.id == v.id_definicion && d.tiene_maximo));
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
        None => {
            informe.partida_nueva = true;
            EstadoPartida::default()
        }
        Some(json) => match serde_json::from_str::<EstadoPartida>(&json) {
            Ok(e) => e,
            Err(err) => {
                eprintln!("[partida] partida.json ilegible ({err}); se aparta y se empieza vacío");
                informe.partida_corrupta_respaldada_en = almacen.respaldar_partida_corrupta();
                informe.partida_nueva = true;
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

    // Fichas de versiones anteriores (o anteriores a un cambio de definiciones): un parámetro
    // por definición, y la visibilidad fija aplicada. No es un delta: el resultado no cambia de
    // significado, y se queda guardado en la siguiente compactación.
    estado.ajustar_a_definiciones();

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
                parametros: vec![Parametro { id_definicion: ID_VIDA, valor: ValorParametro::Numero(20), publico: true }],
                cuerpo: Cuerpo::Pequenio,
                zona_id: 1,
                foto_url: None,
                archivos_adjuntos: vec![],
                es_unico: false,
            },
            valores_actuales: vec![ValorActual { id_definicion: ID_VIDA, valor: vida }],
            coordenadas: Coordenada::NoPuesto,
            activo: true,
        }
    }

    fn cordura() -> DefinicionParametro {
        DefinicionParametro {
            id: 5,
            clave: "Cordura".into(),
            valor_por_defecto: ValorParametro::Numero(8),
            tiene_maximo: true,
            representar_mapa: None,
            visibilidad: Visibilidad::Variable,
            unidad: None,
        }
    }

    fn claves(e: &EstadoPartida) -> Vec<&str> {
        e.definiciones_parametros.iter().map(|d| d.clave.as_str()).collect()
    }

    #[test]
    fn crear_borrar_y_reordenar_parametros_llega_a_fichas_e_instancias() {
        let dir = dir_temporal("parametros_cambios");

        {
            let a = Almacen::abrir(&dir).unwrap();
            let mut e = EstadoPartida::default();
            let mut inst = instancia(1, 10);
            inst.base.parametros = vec![];
            e.registrar(&a, DeltaPartida::EntidadCreada(inst.base.clone())).unwrap();
            e.instancias_activas.push(inst);
            e.ajustar_a_definiciones();

            e.registrar(&a, DeltaPartida::ParametroCreado(cordura())).unwrap();
            assert_eq!(e.banco_entidades[0].parametros.len(), 5);
            assert_eq!(e.banco_entidades[0].parametros[4].valor, ValorParametro::Numero(8));

            e.registrar(&a, DeltaPartida::ParametrosReordenados(vec![5, 1, 2, 3, 4])).unwrap();
            assert_eq!(claves(&e), ["Cordura", "Vida", "Armadura", "Velocidad", "Rango arma"]);
            let ids_fichas: Vec<u16> = e.banco_entidades[0].parametros.iter().map(|p| p.id_definicion).collect();
            assert_eq!(ids_fichas, [5, 1, 2, 3, 4], "la ficha sigue el orden de las definiciones");

            // La instancia tenía vida actual: al borrar la vida desaparece también su valor actual
            e.instancias_activas[0].valores_actuales.push(ValorActual { id_definicion: ID_VIDA, valor: 7 });
            e.registrar(&a, DeltaPartida::ParametroBorrado(ID_VIDA)).unwrap();
            assert_eq!(claves(&e), ["Cordura", "Armadura", "Velocidad", "Rango arma"]);
            assert!(e.banco_entidades[0].parametros.iter().all(|p| p.id_definicion != ID_VIDA));
            assert!(e.instancias_activas[0].base.parametros.iter().all(|p| p.id_definicion != ID_VIDA));
            assert!(e.instancias_activas[0].valores_actuales.is_empty());
        }

        // Recargar desde el log reproduce lo mismo
        let (e, informe) = cargar(&Almacen::abrir(&dir).unwrap()).unwrap();
        assert_eq!(informe.deltas_aplicados, 4);
        assert_eq!(claves(&e), ["Cordura", "Armadura", "Velocidad", "Rango arma"]);
        assert_eq!(e.banco_entidades[0].parametros.len(), 4);
    }

    #[test]
    fn modificar_un_parametro_conserva_valores_y_ajusta_visibilidad_y_maximo() {
        let a = Almacen::abrir(dir_temporal("parametro_modificado")).unwrap();
        let mut e = EstadoPartida::default();
        let mut inst = instancia(1, 7);
        inst.base.parametros[0].valor = ValorParametro::Numero(20);
        e.registrar(&a, DeltaPartida::EntidadCreada(inst.base.clone())).unwrap();
        e.instancias_activas.push(inst);

        let mut vida = e.definiciones_parametros[0].clone();
        vida.clave = "Puntos de vida".into();
        vida.tiene_maximo = false; // ya no lleva valor actual aparte
        vida.visibilidad = Visibilidad::SiemprePublico;
        e.registrar(&a, DeltaPartida::ParametroModificado(vida)).unwrap();

        assert_eq!(claves(&e)[0], "Puntos de vida");
        let p = &e.banco_entidades[0].parametros[0];
        assert_eq!(p.valor, ValorParametro::Numero(20), "el valor de la ficha no cambia");
        assert!(p.publico, "la visibilidad fija se aplica");
        assert!(e.instancias_activas[0].valores_actuales.is_empty());
    }

    #[test]
    fn una_partida_recien_creada_conviene_compactarla_para_escribir_sus_parametros() {
        let a = Almacen::abrir(dir_temporal("recien_creada")).unwrap();
        let (e, informe) = cargar(&a).unwrap();
        assert!(informe.partida_nueva && informe.conviene_compactar());
        compactar(&a, &e).unwrap();

        let guardado = a.leer_partida().unwrap().expect("ahora existe partida.json");
        for clave in ["Vida", "Armadura", "Velocidad", "Rango arma"] {
            assert!(guardado.contains(clave), "falta {clave} en partida.json");
        }
        // La siguiente carga ya no es "nueva"
        assert!(!cargar(&a).unwrap().1.conviene_compactar());
    }

    fn vida(e: &EstadoPartida) -> i16 {
        e.instancias_activas[0].valores_actuales[0].valor
    }

    #[test]
    fn el_valor_actual_no_desborda_y_un_parametro_inexistente_no_hace_nada() {
        let mut e = EstadoPartida::default();
        e.instancias_activas.push(instancia(1, 10));
        e.aplicar_delta(&DeltaPartida::ValorActualModificado { id_instancia: 1, id_definicion: ID_VIDA, delta: i16::MIN });
        e.aplicar_delta(&DeltaPartida::ValorActualModificado { id_instancia: 1, id_definicion: ID_VIDA, delta: i16::MIN });
        assert_eq!(vida(&e), i16::MIN);
        e.aplicar_delta(&DeltaPartida::ValorActualModificado { id_instancia: 1, id_definicion: 99, delta: 5 });
        e.aplicar_delta(&DeltaPartida::ValorActualModificado { id_instancia: 9, id_definicion: ID_VIDA, delta: 5 });
        assert_eq!(vida(&e), i16::MIN);
    }

    #[test]
    fn una_partida_nueva_o_antigua_tiene_los_parametros_de_serie() {
        assert_eq!(EstadoPartida::default().definiciones_parametros.len(), 4);

        let a = Almacen::abrir(dir_temporal("sin_parametros")).unwrap();
        a.escribir_partida(r#"{"ultimo_seq":0,"zonas":[{"id":1,"nombre":"Z","orden":0}]}"#).unwrap();
        let (e, _) = cargar(&a).unwrap();
        let claves: Vec<_> = e.definiciones_parametros.iter().map(|d| d.clave.as_str()).collect();
        assert_eq!(claves, ["Vida", "Armadura", "Velocidad", "Rango arma"]);
    }

    #[test]
    fn una_ficha_antigua_en_el_log_se_carga_con_sus_valores() {
        let a = Almacen::abrir(dir_temporal("ficha_antigua")).unwrap();
        a.anadir_delta(
            r#"{"seq":1,"delta":{"EntidadCreada":{"id":2,"nombre":"Lobo","tipo":"Enemigo","vida_max":9,
            "vida_es_publica":false,"armadura":1,"armadura_es_publica":true,"velocidad":12,"rango_arma":1,
            "cuerpo":"Pequenio","zona_id":1}}}"#
                .replace('\n', " ")
                .as_str(),
        )
        .unwrap();
        let (e, informe) = cargar(&a).unwrap();
        assert_eq!(informe.lineas_invalidas, 0);
        let lobo = &e.banco_entidades[0];
        assert_eq!(lobo.parametros.len(), 4);
        assert_eq!(lobo.parametros[0].valor, ValorParametro::Numero(9));
        assert!(lobo.parametros[1].publico, "la armadura era pública");
        assert_eq!(lobo.parametros[2].valor, ValorParametro::Numero(12));
        assert!(lobo.parametros[2].publico, "la velocidad es siempre pública");
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
    fn al_agrandar_la_rejilla_una_ficha_que_queda_fuera_vuelve_al_borde() {
        let mut e = EstadoPartida::default();
        e.aplicar_delta(&DeltaPartida::MapaCreado(zona(1), mapa(1, 1))); // 1000 px / 50 = 20×20
        let mut grande = instancia(1, 5);
        grande.id_mapa = Some(1);
        grande.base.cuerpo = Cuerpo::Grande(vec![3, 3]);
        grande.coordenadas = Coordenada::Casilla(Casilla { x: 18, y: 10 });
        let mut otra = instancia(2, 5); // en otro mapa: no se toca
        otra.id_mapa = Some(2);
        otra.coordenadas = Coordenada::Casilla(Casilla { x: 18, y: 18 });
        e.instancias_activas = vec![grande, otra];

        // 1000 / 100 = 10×10: el 3×3 en x=18 se sale y pasa a x=8 (ocupa 7..9)
        e.aplicar_delta(&DeltaPartida::MapaModificado(Mapa { rejilla: 100, ..mapa(1, 1) }));
        assert_eq!(e.instancias_activas[0].coordenadas, Coordenada::Casilla(Casilla { x: 8, y: 8 }));
        assert_eq!(e.instancias_activas[1].coordenadas, Coordenada::Casilla(Casilla { x: 18, y: 18 }));
    }

    #[test]
    fn un_corte_entre_snapshot_y_vaciado_no_duplica_deltas() {
        let a = Almacen::abrir(dir_temporal("corte")).unwrap();
        let mut e = EstadoPartida::default();
        e.instancias_activas.push(instancia(1, 10));
        e.registrar(&a, DeltaPartida::ValorActualModificado { id_instancia: 1, id_definicion: ID_VIDA, delta: -3 })
            .unwrap();
        assert_eq!(vida(&e), 7);

        // Simula el corte: el snapshot se escribe, pero deltas.jsonl NO llega a vaciarse.
        a.escribir_partida(&serde_json::to_string(&e).unwrap()).unwrap();

        let (e2, informe) = cargar(&a).unwrap();
        assert_eq!(vida(&e2), 7, "la vida no debe bajar dos veces");
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
