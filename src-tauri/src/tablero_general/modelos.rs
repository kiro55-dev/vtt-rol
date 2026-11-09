// src-tauri/src/tablero_general/modelos.rs
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum TipoEntidad {
    Enemigo,
    NPC,
    Jugador,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Cuerpo {
    Grande(Vec<u8>),
    Pequenio,
}

/// Casilla del tablero, como en una hoja de cálculo: (0,0) arriba a la izquierda, x hacia la
/// derecha, y hacia abajo. La lógica vive en `tablero.rs`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Casilla {
    pub x: u16,
    pub y: u16,
}

/// Dónde está una instancia. En una ficha de varias casillas es la casilla CENTRAL
/// (ver `tablero::extension`), para que pueda girar sobre sí misma.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(from = "CoordenadaGuardada")]
pub enum Coordenada {
    Casilla(Casilla),
    NoPuesto,
}

/// Forma en que se LEE una `Coordenada`: la actual más la antigua `Localizacion([x, y])`.
#[derive(Deserialize)]
enum CoordenadaGuardada {
    Casilla(Casilla),
    NoPuesto,
    Localizacion(Vec<i16>),
}

impl From<CoordenadaGuardada> for Coordenada {
    fn from(g: CoordenadaGuardada) -> Self {
        match g {
            CoordenadaGuardada::Casilla(c) => Coordenada::Casilla(c),
            CoordenadaGuardada::NoPuesto => Coordenada::NoPuesto,
            CoordenadaGuardada::Localizacion(v) => match v[..] {
                [x, y, ..] => Coordenada::Casilla(Casilla { x: x.max(0) as u16, y: y.max(0) as u16 }),
                _ => Coordenada::NoPuesto,
            },
        }
    }
}

// ========================================================
// PARÁMETROS
// ========================================================
//
// Tres niveles:
//  * Partida   -> `DefinicionParametro`: qué parámetros existen (común a todas las fichas).
//  * Ficha     -> `Parametro`: el valor de esa ficha y si se ha hecho público.
//  * Instancia -> `ValorActual`: lo que cambia durante la partida. Solo lo tienen los parámetros
//                 con `tiene_maximo` (p. ej. la vida): el valor de la ficha es el máximo y cada
//                 instancia lleva su valor actual.

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(from = "DefinicionGuardada")]
pub struct DefinicionParametro {
    pub id: u16,
    /// Nombre que se muestra: "Vida", "Armadura", "Cordura"...
    pub clave: String,
    /// Da el tipo (número o texto) y el valor con el que nace una ficha nueva.
    pub valor_por_defecto: ValorParametro,
    /// El valor de la ficha es el máximo; cada instancia guarda su valor actual. Solo números.
    #[serde(default)]
    pub tiene_maximo: bool,
    /// Cómo se dibuja en el tablero al seleccionar la instancia (None = no se dibuja).
    #[serde(default)]
    pub representar_mapa: Option<Forma>,
    pub visibilidad: Visibilidad,
    /// Texto tras el valor, p. ej. "m".
    #[serde(default)]
    pub unidad: Option<String>,
    /// Solo con `tiene_maximo`: si el valor actual de una instancia llega a 0 o menos, la ficha se ve
    /// atenuada (en gris) en el tablero y en «Fichas activas». Es lo que hace la vida.
    pub atenuar_si_negativo: bool,
}

/// Forma en que se LEE una `DefinicionParametro`: las partidas guardadas antes de `atenuar_si_negativo`
/// no lo traen, y su Vida de serie debe seguir atenuando como hasta ahora.
#[derive(Deserialize)]
struct DefinicionGuardada {
    id: u16,
    clave: String,
    valor_por_defecto: ValorParametro,
    #[serde(default)]
    tiene_maximo: bool,
    #[serde(default)]
    representar_mapa: Option<Forma>,
    visibilidad: Visibilidad,
    #[serde(default)]
    unidad: Option<String>,
    #[serde(default)]
    atenuar_si_negativo: Option<bool>,
}

impl From<DefinicionGuardada> for DefinicionParametro {
    fn from(g: DefinicionGuardada) -> Self {
        DefinicionParametro {
            atenuar_si_negativo: g.atenuar_si_negativo.unwrap_or(g.id == ID_VIDA && g.tiene_maximo),
            id: g.id,
            clave: g.clave,
            valor_por_defecto: g.valor_por_defecto,
            tiene_maximo: g.tiene_maximo,
            representar_mapa: g.representar_mapa,
            visibilidad: g.visibilidad,
            unidad: g.unidad,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Visibilidad {
    SiemprePublico,
    SiemprePrivado,
    /// Cada ficha decide (casilla "Hacer pública...").
    Variable,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Forma {
    //en todos los casos vec<u8> representan RGBA
    Rombo(Vec<u8>),
    Circulo(Vec<u8>),
    Cuadrado(Vec<u8>),
    BarraVida(Vec<u8>),
    /// Solo con `tiene_maximo`: en «Fichas activas» se muestra «actual/máx» en este color.
    Resumen(Vec<u8>),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ValorParametro {
    Numero(i16),
    Texto(String),
}

impl ValorParametro {
    pub fn mismo_tipo(&self, otro: &ValorParametro) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(otro)
    }
}

/// Valor de un parámetro en una ficha.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Parametro {
    pub id_definicion: u16,
    pub valor: ValorParametro,
    /// Si los jugadores lo ven. Con visibilidad fija siempre coincide con ella (ver `normalizar_parametros`).
    #[serde(default)]
    pub publico: bool,
}

/// Valor actual, en una instancia, de un parámetro con máximo.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ValorActual {
    pub id_definicion: u16,
    pub valor: i16,
}

// Ids de los parámetros por defecto. Son fijos: la migración de fichas antiguas depende de ellos.
pub const ID_VIDA: u16 = 1;
pub const ID_ARMADURA: u16 = 2;
pub const ID_VELOCIDAD: u16 = 3;
pub const ID_RANGO_ARMA: u16 = 4;

/// Los parámetros con los que nace una partida (los que antes eran campos fijos de `Entidad`).
pub fn parametros_por_defecto() -> Vec<DefinicionParametro> {
    let def = |id, clave: &str, valor, tiene_maximo, representar_mapa, visibilidad, unidad: Option<&str>| {
        DefinicionParametro {
            id,
            clave: clave.into(),
            valor_por_defecto: ValorParametro::Numero(valor),
            tiene_maximo,
            representar_mapa,
            visibilidad,
            unidad: unidad.map(Into::into),
            // La vida: a 0 o menos, la ficha se ve atenuada
            atenuar_si_negativo: id == ID_VIDA,
        }
    };
    vec![
        def(ID_VIDA, "Vida", 10, true, Some(Forma::Resumen(vec![34, 177, 76, 255])), Visibilidad::Variable, None),
        def(ID_ARMADURA, "Armadura", 10, false, None, Visibilidad::Variable, None),
        def(
            ID_VELOCIDAD,
            "Velocidad",
            6,
            false,
            Some(Forma::Rombo(vec![59, 130, 246, 90])),
            Visibilidad::SiemprePublico,
            Some("m"),
        ),
        def(
            ID_RANGO_ARMA,
            "Rango arma",
            1,
            false,
            Some(Forma::Circulo(vec![239, 68, 68, 90])),
            Visibilidad::SiemprePublico,
            Some("m"),
        ),
    ]
}

/// Deja un parámetro por definición y en su orden: añade con su valor por defecto los que faltan,
/// quita los de definiciones que ya no existen (o repetidos) y ajusta `publico` a la visibilidad fija.
pub fn normalizar_parametros(defs: &[DefinicionParametro], parametros: &mut Vec<Parametro>) {
    let mut anteriores = std::mem::take(parametros);
    for d in defs {
        let mut p = match anteriores.iter().position(|p| p.id_definicion == d.id) {
            Some(i) => anteriores.swap_remove(i),
            None => Parametro { id_definicion: d.id, valor: d.valor_por_defecto.clone(), publico: false },
        };
        match d.visibilidad {
            Visibilidad::SiemprePublico => p.publico = true,
            Visibilidad::SiemprePrivado => p.publico = false,
            Visibilidad::Variable => {}
        }
        parametros.push(p);
    }
}

// ========================================================
// JUGADORES
// ========================================================

/// Identifica a un jugador de la partida. Lo asigna el host al registrarlo (`jugadores::registrar`) y se
/// guarda en la partida, así quien vuelve otro día conserva sus fichas.
pub type IdJugador = u16;

/// El host (el máster). En el fondo es un jugador más, pero tiene acceso a todo: nunca hace falta
/// ponerlo en `Entidad.propietarios`.
pub const ID_HOST: IdJugador = 0;

/// Un jugador registrado en la partida (ver `EstadoPartida.jugadores`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Jugador {
    pub id: IdJugador,
    pub nombre: String,
}

// ========================================================
// ENTIDADES E INSTANCIAS
// ========================================================

pub const ZOOM_MIN: u16 = 100;
pub const ZOOM_MAX: u16 = 400;

/// Cómo se encuadra la foto de una ficha en su miniatura (lo elige el usuario en el editor, que recorta
/// el WebP y lo manda a Rust). `zoom` en % (100 = la foto cubre justo el cuadrado, hasta `ZOOM_MAX`);
/// `x` e `y` (0-100) eligen qué parte de lo que sobra se ve: 0 = arriba/izquierda, 50 = centro. Se guarda
/// para que, al volver a abrir la ficha, la foto aparezca como se dejó y se pueda seguir moviendo.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncuadreFoto {
    pub x: u8,
    pub y: u8,
    pub zoom: u16,
}

impl Default for EncuadreFoto {
    fn default() -> Self {
        EncuadreFoto { x: 50, y: 50, zoom: ZOOM_MIN }
    }
}

impl EncuadreFoto {
    pub fn es_valido(&self) -> bool {
        self.x <= 100 && self.y <= 100 && (ZOOM_MIN..=ZOOM_MAX).contains(&self.zoom)
    }
}

// Representa la definición de una entidad
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(from = "EntidadGuardada")]
pub struct Entidad {
    pub id: u16,
    pub nombre: String,
    pub tipo: TipoEntidad,
    /// Valores de los parámetros de la partida, en el orden de sus definiciones.
    pub parametros: Vec<Parametro>,
    pub cuerpo: Cuerpo,
    pub zona_id: u16,

    // Una criatura o NPC puede no tener foto (en JS llegará como string o null)
    pub foto_url: Option<String>,

    /// Miniatura propia de la ficha (`miniaturas/ficha/entidad_<id>_<versión>.webp`): la que el usuario
    /// encuadró en el editor; el front la recorta y Rust la guarda tal cual. La pone solo el delta
    /// `MiniaturaFichaCambiada`; lo que mande el front en `modificar_entidad` se ignora. Sin ella, todo
    /// usa la miniatura automática de `foto_url`.
    pub miniatura_url: Option<String>,
    /// Encuadre con el que se hizo esa miniatura (ver `EncuadreFoto`).
    pub foto_encuadre: EncuadreFoto,

    /* Concepto incorrecto debido a que prefiero que sea modular lo que es publico y que no
    * Además, una ficha (excepto si es jugador) como concepto es público si tiene una instancia
    *pub es_publico: bool,
    */
    pub archivos_adjuntos: Vec<Option<String>>,

    pub es_unico: bool,

    /// Jugadores que controlan la ficha (la ven entera, la editan, mueven sus instancias...), sin el host,
    /// que siempre puede todo. Vacío = solo el host. Ver `jugadores.rs` y `visibilidad.rs`.
    pub propietarios: Vec<IdJugador>,
}

/// Forma en que se LEE una `Entidad`: la actual más los campos fijos de versiones anteriores
/// (vida_max, armadura...), que se pasan a `parametros`. Así se cargan las partidas antiguas,
/// tanto en partida.json como en las líneas de deltas.jsonl.
#[derive(Deserialize)]
struct EntidadGuardada {
    id: u16,
    nombre: String,
    tipo: TipoEntidad,
    #[serde(default)]
    parametros: Vec<Parametro>,
    cuerpo: Cuerpo,
    zona_id: u16,
    #[serde(default)]
    foto_url: Option<String>,
    #[serde(default)]
    miniatura_url: Option<String>,
    #[serde(default)]
    foto_encuadre: EncuadreFoto,
    // Si falta en el JSON, se asume lista vacía []
    #[serde(default)]
    archivos_adjuntos: Vec<Option<String>>,
    #[serde(default)]
    es_unico: bool,
    #[serde(default)]
    propietarios: Vec<IdJugador>,

    // Formato antiguo
    vida_max: Option<i16>,
    vida_es_publica: Option<bool>,
    armadura: Option<i16>,
    armadura_es_publica: Option<bool>,
    velocidad: Option<i16>,
    rango_arma: Option<i16>,
}

impl From<EntidadGuardada> for Entidad {
    fn from(g: EntidadGuardada) -> Self {
        let mut parametros = g.parametros;
        if parametros.is_empty() {
            let antiguos = [
                (ID_VIDA, g.vida_max, g.vida_es_publica),
                (ID_ARMADURA, g.armadura, g.armadura_es_publica),
                (ID_VELOCIDAD, g.velocidad, None),
                (ID_RANGO_ARMA, g.rango_arma, None),
            ];
            for (id_definicion, valor, publico) in antiguos {
                if let Some(v) = valor {
                    parametros.push(Parametro {
                        id_definicion,
                        valor: ValorParametro::Numero(v),
                        publico: publico.unwrap_or(false),
                    });
                }
            }
        }
        Entidad {
            id: g.id,
            nombre: g.nombre,
            tipo: g.tipo,
            parametros,
            cuerpo: g.cuerpo,
            zona_id: g.zona_id,
            foto_url: g.foto_url,
            miniatura_url: g.miniatura_url,
            foto_encuadre: g.foto_encuadre,
            archivos_adjuntos: g.archivos_adjuntos,
            es_unico: g.es_unico,
            propietarios: g.propietarios,
        }
    }
}

// Representa una instancia de una entidad en el mapa
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Instancia {
    pub id_instancia: u16,
    pub id_mapa: Option<u16>,
    pub base: Entidad,

    /// Valor actual de los parámetros con máximo (p. ej. la vida); el máximo es el de `base`.
    /// Si la base no lo tiene público, no se comparte.
    #[serde(default)]
    pub valores_actuales: Vec<ValorActual>,
    pub coordenadas: Coordenada,

    #[serde(default)]
    pub activo: bool,

    /// Cuartos de vuelta en el sentido del reloj (0..=3). Con uno impar, ancho y alto del cuerpo se
    /// intercambian (ver `tablero::lados_instancia`); el centro no se mueve.
    #[serde(default)]
    pub rotacion: u8,

    /// La imagen se dibuja en espejo (de izquierda a derecha, en el eje de la propia ficha, antes de girar).
    /// Solo es de aspecto: no cambia los lados ni la posición.
    #[serde(default)]
    pub espejo: bool,

    /// Posición en «Fichas activas» (de arriba abajo). Ver `DeltaPartida::InstanciasReordenadas`.
    #[serde(default)]
    pub orden: u16,

    /// Altura en el tablero: con fichas solapadas, la de capa mayor se dibuja encima. Al poner o mover
    /// una ficha en el tablero recibe la mayor (ver `instancias::capa_superior`).
    #[serde(default)]
    pub capa: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Zona {
    pub id: u16,
    pub nombre: String,

    // Si no viene en el JSON, se asume 0
    #[serde(default)]
    pub orden: u16,
}

// El mapa relacionado con una zona
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Mapa {
    pub id: u16,
    pub id_zona: u16,
    #[serde(default)]
    pub nombre: String,
    // El mapa puede no tener imagen asignada (en JS llegará como string o null)
    #[serde(default)]
    pub url_imagen_fondo: Option<String>,

    pub ancho: u16,
    pub alto: u16,
    pub rejilla: u16,
    pub color_opacidad: Vec<u8>,
    pub grosor: u16,

    // Si no viene en el JSON, se asume 0
    #[serde(default)]
    pub orden: u16,

    /// Lo ven los jugadores de la sala (la casilla de arriba a la derecha del mapa). Lo cambia solo el delta
    /// `MapaCompartido`: `modificar_mapa` conserva el guardado.
    #[serde(default)]
    pub compartido: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Tiradas {
    pub total: i32,
    pub comando: String,
    pub hora: String,
    /// Quién tiró. Por ahora siempre el host; las tiradas guardadas antes del campo también son suyas.
    #[serde(default)]
    pub jugador: IdJugador,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ArchivoAdjunto {
    pub ruta: String,
    pub entidad_archivo: Entidad,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numeros(ps: &[Parametro]) -> Vec<(u16, ValorParametro, bool)> {
        ps.iter().map(|p| (p.id_definicion, p.valor.clone(), p.publico)).collect()
    }

    #[test]
    fn una_ficha_antigua_pasa_sus_campos_a_parametros() {
        let json = r#"{"id":3,"nombre":"Orco","tipo":"Enemigo","vida_max":18,"vida_es_publica":true,
            "armadura":4,"armadura_es_publica":false,"velocidad":9,"rango_arma":2,"cuerpo":"Pequenio",
            "zona_id":1,"foto_url":null,"archivos_adjuntos":[],"es_unico":false}"#;
        let e: Entidad = serde_json::from_str(json).unwrap();
        use ValorParametro::Numero;
        assert_eq!(
            numeros(&e.parametros),
            vec![
                (ID_VIDA, Numero(18), true),
                (ID_ARMADURA, Numero(4), false),
                (ID_VELOCIDAD, Numero(9), false),
                (ID_RANGO_ARMA, Numero(2), false),
            ]
        );
        // Al volver a guardarla ya no lleva los campos antiguos
        let guardada = serde_json::to_string(&e).unwrap();
        assert!(!guardada.contains("vida_max"), "{guardada}");
        let releida: Entidad = serde_json::from_str(&guardada).unwrap();
        assert_eq!(releida.parametros, e.parametros);
    }

    #[test]
    fn el_encuadre_de_la_foto_se_guarda_y_las_fichas_antiguas_salen_centradas() {
        // Sin el campo (partida anterior): centrada y sin zoom
        let antigua = r#"{"id":3,"nombre":"Orco","tipo":"Enemigo","parametros":[],"cuerpo":"Pequenio",
            "zona_id":1,"foto_url":null,"archivos_adjuntos":[],"es_unico":false}"#;
        let e: Entidad = serde_json::from_str(antigua).unwrap();
        assert_eq!(e.foto_encuadre, EncuadreFoto { x: 50, y: 50, zoom: 100 });
        assert_eq!(e.miniatura_url, None);

        // Con encuadre: se conserva al guardar y releer
        let mut e = e;
        e.foto_encuadre = EncuadreFoto { x: 10, y: 80, zoom: 250 };
        let releida: Entidad = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert_eq!(releida.foto_encuadre, EncuadreFoto { x: 10, y: 80, zoom: 250 });

        assert!(EncuadreFoto::default().es_valido());
        assert!(!EncuadreFoto { x: 101, y: 0, zoom: 100 }.es_valido());
        assert!(!EncuadreFoto { x: 0, y: 0, zoom: 99 }.es_valido());
        assert!(!EncuadreFoto { x: 0, y: 0, zoom: 401 }.es_valido());
    }

    #[test]
    fn una_ficha_sin_propietarios_es_solo_del_host_y_los_conserva_al_guardar() {
        let antigua = r#"{"id":3,"nombre":"Orco","tipo":"Enemigo","parametros":[],"cuerpo":"Pequenio",
            "zona_id":1,"foto_url":null,"archivos_adjuntos":[],"es_unico":false}"#;
        let mut e: Entidad = serde_json::from_str(antigua).unwrap();
        assert!(e.propietarios.is_empty());

        e.propietarios = vec![2, 5];
        let releida: Entidad = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert_eq!(releida.propietarios, vec![2, 5]);

        // Las tiradas guardadas antes de saber quién tiraba son del host
        let t: Tiradas = serde_json::from_str(r#"{"total":3,"comando":"1d6 (3)","hora":"10:00"}"#).unwrap();
        assert_eq!(t.jugador, ID_HOST);
    }

    #[test]
    fn normalizar_rellena_ordena_quita_y_ajusta_la_visibilidad_fija() {
        let defs = parametros_por_defecto();
        let mut ps = vec![
            Parametro { id_definicion: ID_RANGO_ARMA, valor: ValorParametro::Numero(5), publico: false },
            Parametro { id_definicion: 99, valor: ValorParametro::Numero(1), publico: true },
            Parametro { id_definicion: ID_VIDA, valor: ValorParametro::Numero(30), publico: true },
            Parametro { id_definicion: ID_VIDA, valor: ValorParametro::Numero(1), publico: false },
        ];
        normalizar_parametros(&defs, &mut ps);
        use ValorParametro::Numero;
        assert_eq!(
            numeros(&ps),
            vec![
                (ID_VIDA, Numero(30), true),
                (ID_ARMADURA, Numero(10), false),
                (ID_VELOCIDAD, Numero(6), true),
                (ID_RANGO_ARMA, Numero(5), true),
            ]
        );
    }

    #[test]
    fn solo_la_vida_de_serie_atenua_y_las_partidas_antiguas_la_conservan() {
        let defs = parametros_por_defecto();
        let atenuan: Vec<_> = defs.iter().filter(|d| d.atenuar_si_negativo).map(|d| d.id).collect();
        assert_eq!(atenuan, vec![ID_VIDA]);

        // Una definición guardada antes de existir el campo: la Vida atenúa, lo demás no
        let vieja = |id: u16, max: bool| -> DefinicionParametro {
            serde_json::from_str(&format!(
                r#"{{"id":{id},"clave":"X","valor_por_defecto":{{"Numero":5}},"tiene_maximo":{max},"visibilidad":"Variable"}}"#
            ))
            .unwrap()
        };
        assert!(vieja(ID_VIDA, true).atenuar_si_negativo);
        assert!(!vieja(ID_VIDA, false).atenuar_si_negativo);
        assert!(!vieja(7, true).atenuar_si_negativo);

        // Si está guardado, manda lo guardado (aunque sea la Vida desmarcada)
        let json = serde_json::to_string(&DefinicionParametro { atenuar_si_negativo: false, ..vieja(ID_VIDA, true) }).unwrap();
        assert!(!serde_json::from_str::<DefinicionParametro>(&json).unwrap().atenuar_si_negativo);
    }

    #[test]
    fn las_coordenadas_antiguas_pasan_a_casilla() {
        let leer = |j: &str| serde_json::from_str::<Coordenada>(j).unwrap();
        assert_eq!(leer(r#"{"Localizacion":[3,4]}"#), Coordenada::Casilla(Casilla { x: 3, y: 4 }));
        assert_eq!(leer(r#"{"Localizacion":[-1,2]}"#), Coordenada::Casilla(Casilla { x: 0, y: 2 }));
        assert_eq!(leer(r#"{"Localizacion":[7]}"#), Coordenada::NoPuesto);
        assert_eq!(leer(r#""NoPuesto""#), Coordenada::NoPuesto);

        // El formato nuevo va y vuelve igual
        let nueva = Coordenada::Casilla(Casilla { x: 5, y: 1 });
        let json = serde_json::to_string(&nueva).unwrap();
        assert_eq!(json, r#"{"Casilla":{"x":5,"y":1}}"#);
        assert_eq!(leer(&json), nueva);
    }
}
