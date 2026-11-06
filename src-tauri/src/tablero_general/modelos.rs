// src-tauri/src/tablero_general/modelos.rs
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum TipoEntidad {
    Enemigo,
    NPC,
    Jugador,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Cuerpo {
    Grande(Vec<u8>),
    Pequenio,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Coordenada {
    Localizacion(Vec<i16>),
    NoPuesto,
}

// Representa la definición de una entidad
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Entidad {
    pub id: u16,
    pub nombre: String,
    pub tipo: TipoEntidad,
    pub vida_max: i16,
    #[serde(default)]
    pub vida_es_publica: bool,
    pub armadura: i16,
    #[serde(default)]
    pub armadura_es_publica: bool,
    pub velocidad: i16,
    pub rango_arma: i16,
    pub cuerpo: Cuerpo,
    pub zona_id: u16,

    // Una criatura o NPC puede no tener foto (en JS llegará como string o null)
    #[serde(default)]
    pub foto_url: Option<String>,

    /* Concepto incorrecto debido a que prefiero que sea modular lo que es publico y que no
    * Además, una ficha (excepto si es jugador) como concepto es público si tiene una instancia
    *#[serde(default)]
    *pub es_publico: bool,
    */
    // Si falta en el JSON, se asume lista vacía []
    #[serde(default)]
    pub archivos_adjuntos: Vec<Option<String>>,

    #[serde(default)]
    pub es_unico: bool,
}

// Representa una instancia de una entidad en el mapa
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Instancia {
    pub id_instancia: u16,
    pub id_mapa: Option<u16>,
    pub base: Entidad,

    //Si la base tiene que la vida no es pública no se comparte este parámetro
    pub vida_actual: i16,
    pub coordenadas: Coordenada,

    #[serde(default)]
    pub activo: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Zona {
    pub id: u16,
    pub nombre: String,

    // Si no viene en el JSON, se asume 0
    #[serde(default)]
    pub orden: u16,
}

// El mapa relacionado con una zona
#[derive(Serialize, Deserialize, Clone, Debug)]
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
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Tiradas {
    pub total: i32,
    pub comando: String,
    pub hora: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ArchivoAdjunto {
    pub ruta: String,
    pub entidad_archivo: Entidad,
}