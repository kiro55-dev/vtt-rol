// src-tauri/src/red/peticiones.rs
//
// Lo que un jugador puede pedirle al host. Cada acción del jugador en su navegador llega como una
// `Peticion`; el host comprueba que puede hacerla (`permitida`, pura y con tests, sobre `jugadores.rs`) y la
// ejecuta con LA MISMA función del comando de Tauri que usa el host (sin duplicar reglas): se registra su
// delta como cualquier otro y `Red::tras_cambio` lo envía a todos.
//
//  * Solo existe lo que está en la lista blanca (`Peticion`): cualquier otra cosa no se decodifica.
//  * El jugador solo toca lo suyo: sus fichas (`Entidad.propietarios`) y sus instancias, y solo lleva fichas
//    a mapas compartidos. El host no pasa por aquí: sus acciones van directas a Rust y no tienen esos límites.
//  * Los archivos de una ficha son solo `.txt` dentro de `archivos/entidad_<id>/` (`adjunto_permitido_a_jugador`).
use std::collections::HashSet;

use serde::Deserialize;
use serde_json::Value;
use tauri::{AppHandle, Manager, Runtime};


use crate::red::Red;
use crate::tablero_general::cambios_entidad::{self, CambioEntidad};
use crate::tablero_general::dados::tirar_dado_de;
use crate::tablero_general::estado::EstadoPartida;
use crate::tablero_general::instancias::Destino;
use crate::tablero_general::jugadores::{
    adjunto_permitido_a_jugador, cambio_permitido, controla_entidad, controla_instancia, MAX_TEXTO_JUGADOR,
};
use crate::tablero_general::modelos::{normalizar_parametros, ArchivoAdjunto, Entidad, IdJugador, ID_HOST};
use crate::tablero_general::partida::{self, AppState};
use crate::tablero_general::tablero::AvisoTablero;

/// Máximo de fichas o instancias en una sola petición (arrastrar un grupo).
const MAX_GRUPO: usize = 100;
/// Tamaño máximo de la miniatura que sube un jugador (un WebP de 128 px pesa unos KB).
const MAX_MINIATURA: usize = 256 * 1024;
const MAX_NOMBRE_ARCHIVO: usize = 80;
/// Fichas que puede tener un jugador (las que crea él; el host puede darle más).
const MAX_FICHAS_PROPIAS: usize = 20;

/// La lista blanca. Las variantes se llaman igual que los comandos de Tauri y los campos igual que sus
/// argumentos en JS, así el `invoke(cmd, args)` del navegador se envía tal cual: `{ cmd, ...args }`.
#[derive(Deserialize, Debug, Clone)]
#[serde(tag = "cmd", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum Peticion {
    TirarDado { comando: String },
    /// Sus comandos de dados anteriores (las flechas ↑ ↓ al reconectar).
    ObtenerHistorialPersonal,
    MoverInstancia { id_instancia: u16, destino: Option<Destino> },
    MoverInstancias { ids: Vec<u16>, id_mapa: u16, dx: i32, dy: i32 },
    RotarInstancia { id_instancia: u16 },
    EspejarInstancia { id_instancia: u16 },
    BorrarInstancia { id_instancia: u16 },
    ModificarValorActual { id_instancia: u16, id_definicion: u16, delta: i16 },
    InstanciarEntidad { id_entidad: u16, destino: Option<Destino> },
    InstanciarEntidades { ids: Vec<u16>, destino: Option<Destino> },
    ModificarEntidad { entidad: Entidad },
    GuardarMiniaturaFicha { id: u16, datos: Vec<u8> },
    IniciarBorradorEntidad { zona_id: u16, foto_url: Option<String> },
    CrearEntidad { entidad: Entidad },
    CancelarBorradorEntidad { id: u16, rutas: Vec<String> },
    LeerArchivoTexto { ruta_relativa: String },
    GuardarArchivoTexto { adjunto: ArchivoAdjunto, contenido: String },
    CrearDocumentoTexto { entidad: Entidad },
    RenombrarArchivoAdjunto { adjunto: ArchivoAdjunto, nuevo_nombre: String },
    DescartarArchivosBorrador { rutas: Vec<String> },
    AvisarTablero { aviso: AvisoTablero },
}

/// Lo que mandó el navegador (`{ cmd, ...args }`) como una petición de la lista blanca.
pub fn decodificar(v: Value) -> Result<Peticion, String> {
    serde_json::from_value(v).map_err(|_| "Esa acción no está permitida".to_string())
}

// ------------------------------------------------------------------
// Permisos
// ------------------------------------------------------------------

fn instancia_suya(est: &EstadoPartida, j: IdJugador, id: u16) -> Result<(), String> {
    if controla_instancia(est, j, id) { Ok(()) } else { Err("Esa ficha no es tuya".into()) }
}

fn ficha_suya(est: &EstadoPartida, j: IdJugador, id: u16) -> Result<(), String> {
    if controla_entidad(est, j, id) { Ok(()) } else { Err("Esa ficha no es tuya".into()) }
}

fn mapa_compartido(est: &EstadoPartida, id_mapa: u16) -> Result<(), String> {
    if est.mapas.iter().any(|m| m.id == id_mapa && m.compartido) {
        Ok(())
    } else {
        Err("Ese mapa no está compartido".into())
    }
}

/// El destino de una ficha: fuera del tablero, o una casilla de un mapa compartido.
fn destino_ok(est: &EstadoPartida, d: &Option<Destino>) -> Result<(), String> {
    d.as_ref().map_or(Ok(()), |d| mapa_compartido(est, d.id_mapa))
}

fn grupo_ok(n: usize) -> Result<(), String> {
    if n <= MAX_GRUPO { Ok(()) } else { Err("Demasiadas fichas a la vez".into()) }
}

/// De qué ficha es un archivo (`archivos/entidad_<id>/<nombre>.txt`): solo un `.txt` en la carpeta de una ficha
/// del jugador. Devuelve el id de la ficha.
fn archivo_de_ficha_suya(est: &EstadoPartida, j: IdJugador, ruta: &str, borradores: &HashSet<u16>) -> Result<u16, String> {
    let resto = ruta.strip_prefix("archivos/entidad_").ok_or("Ese archivo no es de una ficha tuya")?;
    let (id, nombre) = resto.split_once('/').ok_or("Ese archivo no es de una ficha tuya")?;
    let id: u16 = id.parse().map_err(|_| "Ese archivo no es de una ficha tuya")?;
    if nombre.is_empty() || nombre.contains(['/', '\\']) {
        return Err("Ruta de archivo no válida".into());
    }
    if !borradores.contains(&id) {
        ficha_suya(est, j, id)?;
    }
    if !adjunto_permitido_a_jugador(ruta) {
        return Err("Solo puedes usar archivos de texto (.txt)".into());
    }
    Ok(id)
}

/// Zonas que ve un jugador: las que tienen algún mapa compartido.
fn zona_visible(est: &EstadoPartida, id_zona: u16) -> bool {
    est.mapas.iter().any(|m| m.id_zona == id_zona && m.compartido)
}

/// La zona de una ficha nueva: la pedida si el jugador la ve; si no, la primera que ve.
fn zona_para_ficha_nueva(est: &EstadoPartida, pedida: u16) -> Result<u16, String> {
    if zona_visible(est, pedida) {
        return Ok(pedida);
    }
    est.zonas.iter().map(|z| z.id).find(|z| zona_visible(est, *z)).ok_or_else(|| "El host aún no ha compartido ningún mapa".to_string())
}

fn fichas_de(est: &EstadoPartida, j: IdJugador) -> usize {
    est.banco_entidades.iter().filter(|e| e.propietarios.contains(&j)).count()
}

/// ¿Puede `jugador` hacer esto? `subidas` son las imágenes que ha subido en esta sala.
#[cfg(test)]
pub fn permitida(est: &EstadoPartida, jugador: IdJugador, p: &Peticion, subidas: &HashSet<String>) -> Result<(), String> {
    permitida_con(est, jugador, p, subidas, &HashSet::new())
}

/// Como `permitida`, y `borradores` son las fichas nuevas que tiene abiertas (su id está reservado, aún no están en el banco).
pub fn permitida_con(est: &EstadoPartida, jugador: IdJugador, p: &Peticion, subidas: &HashSet<String>, borradores: &HashSet<u16>) -> Result<(), String> {
    if jugador == ID_HOST {
        return Ok(());
    }
    match p {
        Peticion::TirarDado { .. } | Peticion::ObtenerHistorialPersonal => Ok(()), // cualquiera tira; `tirar_dado_de` rechaza el `clean`
        Peticion::MoverInstancia { id_instancia, destino } => {
            instancia_suya(est, jugador, *id_instancia)?;
            destino_ok(est, destino)
        }
        Peticion::MoverInstancias { ids, id_mapa, .. } => {
            grupo_ok(ids.len())?;
            mapa_compartido(est, *id_mapa)?;
            ids.iter().try_for_each(|id| instancia_suya(est, jugador, *id))
        }
        Peticion::RotarInstancia { id_instancia }
        | Peticion::EspejarInstancia { id_instancia }
        | Peticion::BorrarInstancia { id_instancia }
        | Peticion::ModificarValorActual { id_instancia, .. } => instancia_suya(est, jugador, *id_instancia),
        Peticion::InstanciarEntidad { id_entidad, destino } => {
            ficha_suya(est, jugador, *id_entidad)?;
            destino_ok(est, destino)
        }
        Peticion::InstanciarEntidades { ids, destino } => {
            grupo_ok(ids.len())?;
            destino_ok(est, destino)?;
            ids.iter().try_for_each(|id| ficha_suya(est, jugador, *id))
        }
        Peticion::ModificarEntidad { entidad } => entidad_modificable(est, jugador, entidad, subidas),
        Peticion::IniciarBorradorEntidad { zona_id, foto_url } => {
            if borradores.len() >= crate::red::MAX_BORRADORES_POR_JUGADOR {
                return Err("Tienes demasiadas fichas nuevas abiertas".into());
            }
            if fichas_de(est, jugador) + borradores.len() >= MAX_FICHAS_PROPIAS {
                return Err("Has llegado al máximo de fichas".into());
            }
            if let Some(f) = foto_url.as_deref().filter(|f| !f.trim().is_empty()) {
                if !subidas.contains(f) {
                    return Err("Esa foto no la has subido tú".into());
                }
            }
            zona_para_ficha_nueva(est, *zona_id).map(|_| ())
        }
        Peticion::CrearEntidad { entidad } => {
            if !borradores.contains(&entidad.id) {
                return Err("Esa ficha no es una ficha nueva tuya".into());
            }
            if !zona_visible(est, entidad.zona_id) {
                return Err("Esa zona no está disponible".into());
            }
            if let Some(f) = entidad.foto_url.as_deref().filter(|f| !f.trim().is_empty()) {
                if !subidas.contains(f) {
                    return Err("Esa foto no la has subido tú".into());
                }
            }
            for a in entidad.archivos_adjuntos.iter().flatten() {
                if archivo_de_ficha_suya(est, jugador, a, borradores) != Ok(entidad.id) {
                    return Err("Solo puedes adjuntar archivos de texto (.txt) de tu ficha".into());
                }
            }
            Ok(())
        }
        Peticion::CancelarBorradorEntidad { id, rutas } => {
            if !borradores.contains(id) {
                return Err("Esa ficha no es una ficha nueva tuya".into());
            }
            grupo_ok(rutas.len())?;
            for r in rutas {
                if !subidas.contains(r) && archivo_de_ficha_suya(est, jugador, r, borradores).is_err() {
                    return Err("Ese archivo no es tuyo".into());
                }
            }
            Ok(())
        }
        Peticion::GuardarMiniaturaFicha { id, datos } => {
            ficha_suya(est, jugador, *id)?;
            if datos.len() > MAX_MINIATURA { Err("La miniatura es demasiado grande".into()) } else { Ok(()) }
        }
        Peticion::LeerArchivoTexto { ruta_relativa } => archivo_de_ficha_suya(est, jugador, ruta_relativa, borradores).map(|_| ()),
        Peticion::GuardarArchivoTexto { adjunto, contenido } => {
            if contenido.len() > MAX_TEXTO_JUGADOR {
                return Err("El texto es demasiado largo".into());
            }
            archivo_de_ficha_suya(est, jugador, &adjunto.ruta, borradores).map(|_| ())
        }
        Peticion::CrearDocumentoTexto { entidad } => {
            if borradores.contains(&entidad.id) { Ok(()) } else { ficha_suya(est, jugador, entidad.id) }
        }
        Peticion::RenombrarArchivoAdjunto { adjunto, nuevo_nombre } => {
            if nuevo_nombre.chars().count() > MAX_NOMBRE_ARCHIVO || nuevo_nombre.contains(['/', '\\', ':']) {
                return Err("Nombre de archivo no válido".into());
            }
            archivo_de_ficha_suya(est, jugador, &adjunto.ruta, borradores).map(|_| ())
        }
        Peticion::DescartarArchivosBorrador { rutas } => {
            grupo_ok(rutas.len())?;
            // Solo lo suyo: archivos de sus fichas o imágenes que él subió (nunca borra lo que la partida usa)
            for r in rutas {
                if !subidas.contains(r) && archivo_de_ficha_suya(est, jugador, r, borradores).is_err() {
                    return Err("Ese archivo no es tuyo".into());
                }
            }
            Ok(())
        }
        Peticion::AvisarTablero { aviso } => {
            // El foco de un jugador no se comparte, pero no es un error
            if let Some(id) = aviso.id_mapa() {
                mapa_compartido(est, id)?;
            }
            // El arrastre solo puede nombrar fichas suyas (los demás verían moverse la de otro)
            match aviso {
                AvisoTablero::Arrastre { piezas, .. } => {
                    grupo_ok(piezas.len())?;
                    piezas.iter().try_for_each(|p| instancia_suya(est, jugador, p.id_instancia))
                }
                AvisoTablero::ArrastreFin { ids_instancias, .. } => {
                    grupo_ok(ids_instancias.len())?;
                    ids_instancias.iter().try_for_each(|id| instancia_suya(est, jugador, *id))
                }
                _ => Ok(()),
            }
        }
    }
}

/// Guardar su ficha: solo puede cambiar lo que `cambio_permitido` deja (no de quién es, ni su zona, ni si es
/// única); la foto, solo por una que haya subido; y los adjuntos nuevos, solo .txt de su carpeta.
fn entidad_modificable(est: &EstadoPartida, jugador: IdJugador, e: &Entidad, subidas: &HashSet<String>) -> Result<(), String> {
    ficha_suya(est, jugador, e.id)?;
    let guardada = est.banco_entidades.iter().find(|x| x.id == e.id).ok_or("La ficha ya no existe")?;
    let mut nueva = e.clone();
    normalizar_parametros(&est.definiciones_parametros, &mut nueva.parametros);
    for c in cambios_entidad::entre(guardada, &nueva) {
        if !cambio_permitido(jugador, &c) {
            return Err("Eso de tu ficha solo lo puede cambiar el host".into());
        }
        match &c {
            CambioEntidad::Foto(Some(f)) if !subidas.contains(f) => {
                return Err("Esa foto no la has subido tú".into());
            }
            CambioEntidad::Adjuntos(lista) => {
                for a in lista.iter().flatten() {
                    let ya_estaba = guardada.archivos_adjuntos.iter().flatten().any(|g| g == a);
                    if !ya_estaba && archivo_de_ficha_suya(est, jugador, a, &HashSet::new()) != Ok(e.id) {
                        return Err("Solo puedes adjuntar archivos de texto (.txt) de tu ficha".into());
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

// ------------------------------------------------------------------
// Ejecución
// ------------------------------------------------------------------

fn json<T: serde::Serialize>(r: T) -> Result<Value, String> {
    serde_json::to_value(r).map_err(|e| e.to_string())
}

/// `ejecutar_en` con el estado de la aplicación (lo que usa lib.rs).
pub fn ejecutar<R: Runtime>(app: &AppHandle<R>, red: &Red, jugador: IdJugador, p: Peticion) -> Result<Value, String> {
    ejecutar_en(&app.state::<AppState>(), red, jugador, p)
}

/// Hace lo que pide el jugador, tras comprobar que puede. Llama a la misma función `_en` que el comando de
/// Tauri, así que las reglas son una sola. Devuelve lo que contestaría el comando. Bloqueante (los comandos lo
/// son).
pub fn ejecutar_en(state: &AppState, red: &Red, jugador: IdJugador, p: Peticion) -> Result<Value, String> {
    {
        let est = state.lock(); // estado → red
        permitida_con(&est, jugador, &p, &red.subidas_de(jugador), &red.borradores_de(jugador))?;
    }
    // Solo lee su historial: nada cambia (ni hace falta avisar a nadie)
    if matches!(p, Peticion::ObtenerHistorialPersonal) {
        return json(state.almacen.leer_historial_de(jugador));
    }
    // Qué tiene que volver a leer la pantalla del host: el chat, o las fichas (banco e instancias)
    let partes = if matches!(p, Peticion::TirarDado { .. }) {
        serde_json::json!({ "tiradas": true })
    } else {
        serde_json::json!({ "entidades": true })
    };
    let resultado = match p {
        Peticion::TirarDado { comando } => json(tirar_dado_de(state, jugador, &comando)?),
        Peticion::ObtenerHistorialPersonal => unreachable!("se contesta antes"),
        Peticion::MoverInstancia { id_instancia, destino } => json(partida::mover_instancia_en(id_instancia, destino, state)?),
        Peticion::MoverInstancias { ids, id_mapa, dx, dy } => json(partida::mover_instancias_en(ids, id_mapa, dx, dy, state)?),
        Peticion::RotarInstancia { id_instancia } => json(partida::rotar_instancia_en(id_instancia, state)?),
        Peticion::EspejarInstancia { id_instancia } => json(partida::espejar_instancia_en(id_instancia, state)?),
        Peticion::BorrarInstancia { id_instancia } => json(partida::borrar_instancia_en(id_instancia, state)?),
        Peticion::ModificarValorActual { id_instancia, id_definicion, delta } => {
            json(partida::modificar_valor_actual_en(id_instancia, id_definicion, delta, state)?)
        }
        Peticion::InstanciarEntidad { id_entidad, destino } => json(partida::instanciar_entidad_en(id_entidad, destino, state)?),
        Peticion::InstanciarEntidades { ids, destino } => json(partida::instanciar_entidades_en(ids, destino, state)?),
        Peticion::ModificarEntidad { entidad } => json(partida::modificar_entidad_en(entidad, state)?),
        Peticion::GuardarMiniaturaFicha { id, datos } => json(partida::guardar_miniatura_ficha_en(id, datos, state)?),
        Peticion::LeerArchivoTexto { ruta_relativa } => json(partida::leer_archivo_texto_en(ruta_relativa, state)?),
        Peticion::GuardarArchivoTexto { adjunto, contenido } => {
            let adjunto = con_ficha_guardada(state, adjunto)?;
            json(partida::guardar_archivo_texto_en(adjunto, contenido, state)?)
        }
        Peticion::CrearDocumentoTexto { entidad } => {
            let entidad = ficha_guardada(state, entidad);
            json(partida::crear_documento_texto_en(entidad, state)?)
        }
        Peticion::RenombrarArchivoAdjunto { adjunto, nuevo_nombre } => {
            let adjunto = con_ficha_guardada(state, adjunto)?;
            json(partida::renombrar_archivo_adjunto_en(adjunto, nuevo_nombre, state)?)
        }
        Peticion::DescartarArchivosBorrador { rutas } => json(partida::descartar_archivos_borrador_en(rutas, state)),
        Peticion::IniciarBorradorEntidad { zona_id, foto_url } => {
            let zona = zona_para_ficha_nueva(&state.lock(), zona_id)?;
            let mut borrador = partida::iniciar_borrador_entidad_en(zona, foto_url, state)?;
            if let Err(m) = red.anotar_borrador(jugador, borrador.id) {
                partida::cancelar_borrador_entidad_en(borrador.id, vec![], state);
                return Err(m);
            }
            borrador.propietarios = vec![jugador];
            json(borrador)
        }
        Peticion::CrearEntidad { mut entidad } => {
            // Lo que decide el host, no el jugador: la ficha es suya, no es única y no trae miniatura propia
            entidad.propietarios = vec![jugador];
            entidad.es_unico = false;
            let creada = partida::crear_entidad_en(entidad, state)?;
            red.soltar_borrador(jugador, creada.id);
            json(creada)
        }
        Peticion::CancelarBorradorEntidad { id, rutas } => {
            partida::cancelar_borrador_entidad_en(id, rutas, state);
            red.soltar_borrador(jugador, id);
            json(())
        }
        Peticion::AvisarTablero { aviso } => {
            // El foco de un jugador es suyo: no se envía a nadie (ni al host ni a los demás)
            if aviso.id_mapa().is_none() {
                return Ok(Value::Null);
            }
            {
                let est = state.lock();
                let id = aviso.id_mapa().expect("tiene mapa");
                aviso.validar(est.mapas.iter().find(|m| m.id == id).ok_or("Ese mapa no existe")?)?;
            }
            red.reenviar_aviso(jugador, &aviso);
            return Ok(Value::Null);
        }
    }?;
    // Que los jugadores tengan ya lo último ANTES de la respuesta. Normalmente `registrar` ya se lo envió y esto
    // no repite nada (solo manda lo que cambió desde lo último enviado).
    red.tras_cambio(&state.lock());
    // El host también se entera: su pantalla vuelve a leer lo que cambió
    red.emitir("partida-cambiada", partes);
    Ok(resultado)
}

/// La ficha tal como está guardada (lo que manda el navegador sobre ella no se usa: solo su id). Una ficha nueva
/// aún no está en el banco (`permitida` ya comprobó que es un borrador suyo): se usa la que manda.
fn ficha_guardada(state: &AppState, enviada: Entidad) -> Entidad {
    state.lock().banco_entidades.iter().find(|e| e.id == enviada.id).cloned().unwrap_or(enviada)
}

fn con_ficha_guardada(state: &AppState, adjunto: ArchivoAdjunto) -> Result<ArchivoAdjunto, String> {
    let entidad = ficha_guardada(state, adjunto.entidad_archivo.clone());
    Ok(ArchivoAdjunto { entidad_archivo: entidad, ..adjunto })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tablero_general::modelos::*;
    use serde_json::json;

    fn ficha(id: u16, propietarios: Vec<IdJugador>) -> Entidad {
        Entidad {
            id,
            nombre: format!("F{id}"),
            tipo: TipoEntidad::Jugador,
            parametros: parametros_por_defecto()
                .iter()
                .map(|d| Parametro { id_definicion: d.id, valor: d.valor_por_defecto.clone(), publico: true })
                .collect(),
            cuerpo: Cuerpo::Pequenio,
            zona_id: 1,
            foto_url: Some("imagenes/vieja.png".into()),
            miniatura_url: None,
            foto_encuadre: Default::default(),
            archivos_adjuntos: vec![Some(format!("archivos/entidad_{id}/notas.txt"))],
            es_unico: false,
            propietarios,
        }
    }

    fn instancia(id: u16, base: Entidad, id_mapa: u16) -> Instancia {
        Instancia {
            id_instancia: id, id_mapa: Some(id_mapa), base, valores_actuales: vec![],
            coordenadas: Coordenada::Casilla(Casilla { x: 1, y: 1 }),
            activo: true, rotacion: 0, espejo: false, orden: id, capa: 0, atenuada: false,
        }
    }

    fn mapa(id: u16, compartido: bool) -> Mapa {
        Mapa {
            id, id_zona: 1, nombre: id.to_string(), url_imagen_fondo: None, ancho: 1000, alto: 1000, rejilla: 50,
            color_opacidad: vec![255, 255, 255, 76], grosor: 1, orden: 0, compartido,
        }
    }

    /// Jugador 1 con la ficha 10 (instancia 1); la 11 es del host (instancia 2) y la 12 del jugador 2.
    /// Mapa 1 compartido y mapa 2 no.
    fn estado() -> EstadoPartida {
        let mut e = EstadoPartida::default();
        e.jugadores = vec![Jugador { id: 1, nombre: "Ana".into() }, Jugador { id: 2, nombre: "Luis".into() }];
        e.mapas = vec![mapa(1, true), mapa(2, false)];
        e.banco_entidades = vec![ficha(10, vec![1]), ficha(11, vec![]), ficha(12, vec![2])];
        e.instancias_activas = vec![
            instancia(1, ficha(10, vec![1]), 1),
            instancia(2, ficha(11, vec![]), 1),
            instancia(3, ficha(12, vec![2]), 1),
        ];
        e
    }

    fn pet(v: Value) -> Peticion {
        decodificar(v).unwrap()
    }

    fn ok(est: &EstadoPartida, j: IdJugador, v: Value) -> Result<(), String> {
        permitida(est, j, &pet(v), &HashSet::new())
    }

    fn en(id_mapa: u16) -> Value {
        json!({ "id_mapa": id_mapa, "casilla": { "x": 3, "y": 4 } })
    }

    #[test]
    fn solo_existe_lo_que_esta_en_la_lista_blanca_y_con_los_nombres_del_front() {
        let p = pet(json!({ "cmd": "mover_instancia", "idInstancia": 5, "destino": en(1) }));
        assert!(matches!(p, Peticion::MoverInstancia { id_instancia: 5, destino: Some(_) }));
        assert!(matches!(pet(json!({ "cmd": "mover_instancia", "idInstancia": 5, "destino": null })), Peticion::MoverInstancia { destino: None, .. }));
        for cmd in ["limpiar_chat", "borrar_entidades", "crear_zona", "compartir_mapa", "importar_archivo", "abrir_archivo_en_sistema", "reordenar_instancias", "obtener_zonas", "inventado"] {
            assert!(decodificar(json!({ "cmd": cmd })).is_err(), "{cmd}");
        }
        assert!(decodificar(json!({ "sin": "cmd" })).is_err());
    }

    #[test]
    fn un_jugador_solo_toca_sus_instancias_y_solo_en_mapas_compartidos() {
        let e = estado();
        assert!(ok(&e, 1, json!({ "cmd": "mover_instancia", "idInstancia": 1, "destino": en(1) })).is_ok());
        assert!(ok(&e, 1, json!({ "cmd": "mover_instancia", "idInstancia": 1, "destino": null })).is_ok(), "sacarla del tablero");
        for ajena in [2, 3] {
            assert!(ok(&e, 1, json!({ "cmd": "mover_instancia", "idInstancia": ajena, "destino": en(1) })).is_err(), "{ajena}");
            assert!(ok(&e, 1, json!({ "cmd": "rotar_instancia", "idInstancia": ajena })).is_err());
            assert!(ok(&e, 1, json!({ "cmd": "borrar_instancia", "idInstancia": ajena })).is_err());
            assert!(ok(&e, 1, json!({ "cmd": "modificar_valor_actual", "idInstancia": ajena, "idDefinicion": 1, "delta": -3 })).is_err());
        }
        assert!(ok(&e, 1, json!({ "cmd": "mover_instancia", "idInstancia": 1, "destino": en(2) })).is_err(), "mapa sin compartir");
        assert!(ok(&e, 1, json!({ "cmd": "mover_instancia", "idInstancia": 1, "destino": en(9) })).is_err(), "mapa que no existe");
        for cmd in ["rotar_instancia", "espejar_instancia", "borrar_instancia"] {
            assert!(ok(&e, 1, json!({ "cmd": cmd, "idInstancia": 1 })).is_ok(), "{cmd}");
        }
        assert!(ok(&e, 1, json!({ "cmd": "modificar_valor_actual", "idInstancia": 1, "idDefinicion": 1, "delta": -3 })).is_ok());

        // Varias a la vez: todas suyas y en un mapa compartido
        assert!(ok(&e, 1, json!({ "cmd": "mover_instancias", "ids": [1], "idMapa": 1, "dx": 2, "dy": 0 })).is_ok());
        assert!(ok(&e, 1, json!({ "cmd": "mover_instancias", "ids": [1, 2], "idMapa": 1, "dx": 2, "dy": 0 })).is_err());
        assert!(ok(&e, 1, json!({ "cmd": "mover_instancias", "ids": [1], "idMapa": 2, "dx": 2, "dy": 0 })).is_err());
    }

    #[test]
    fn el_host_no_tiene_esos_limites() {
        let e = estado();
        for id in [1, 2, 3] {
            assert!(ok(&e, ID_HOST, json!({ "cmd": "mover_instancia", "idInstancia": id, "destino": en(2) })).is_ok());
        }
    }

    #[test]
    fn poner_fichas_en_el_tablero_solo_las_suyas_y_en_mapas_compartidos() {
        let e = estado();
        assert!(ok(&e, 1, json!({ "cmd": "instanciar_entidad", "idEntidad": 10, "destino": en(1) })).is_ok());
        assert!(ok(&e, 1, json!({ "cmd": "instanciar_entidad", "idEntidad": 10, "destino": null })).is_ok());
        assert!(ok(&e, 1, json!({ "cmd": "instanciar_entidad", "idEntidad": 11, "destino": null })).is_err(), "del host");
        assert!(ok(&e, 1, json!({ "cmd": "instanciar_entidad", "idEntidad": 10, "destino": en(2) })).is_err());
        assert!(ok(&e, 1, json!({ "cmd": "instanciar_entidades", "ids": [10], "destino": en(1) })).is_ok());
        assert!(ok(&e, 1, json!({ "cmd": "instanciar_entidades", "ids": [10, 12], "destino": null })).is_err());
    }

    #[test]
    fn cualquiera_tira_dados_y_el_chat_se_limpia_solo_desde_el_host() {
        let e = estado();
        assert!(ok(&e, 2, json!({ "cmd": "tirar_dado", "comando": "2d20adv + 3" })).is_ok());
        // `limpiar_chat` no está en la lista blanca; y `clean` como tirada lo rechaza `tirar_dado_de`
        assert!(decodificar(json!({ "cmd": "limpiar_chat", "comando": "clean" })).is_err());
    }

    fn ficha_editada(f: impl FnOnce(&mut Entidad)) -> Value {
        let mut nueva = estado().banco_entidades[0].clone();
        f(&mut nueva);
        json!({ "cmd": "modificar_entidad", "entidad": serde_json::to_value(&nueva).unwrap() })
    }

    #[test]
    fn editar_su_ficha_deja_cambiar_casi_todo_pero_no_de_quien_es_ni_su_zona_ni_si_es_unica() {
        let e = estado();
        assert!(ok(&e, 1, ficha_editada(|f| f.nombre = "Nuevo".into())).is_ok());
        assert!(ok(&e, 1, ficha_editada(|f| f.parametros[1].valor = ValorParametro::Numero(14))).is_ok());
        assert!(ok(&e, 1, ficha_editada(|f| f.cuerpo = Cuerpo::Grande(vec![2, 2]))).is_ok());
        assert!(ok(&e, 1, ficha_editada(|f| f.foto_url = None)).is_ok(), "quitar la foto");

        assert!(ok(&e, 1, ficha_editada(|f| f.zona_id = 4)).is_err());
        assert!(ok(&e, 1, ficha_editada(|f| f.es_unico = true)).is_err());
        assert!(ok(&e, 1, ficha_editada(|f| f.propietarios = vec![1, 2])).is_err(), "no se la puede dar a otro");
        assert!(ok(&e, 1, ficha_editada(|f| f.propietarios = vec![])).is_err());

        // Una ficha ajena, ni abrirla
        let mut ajena = e.banco_entidades[1].clone();
        ajena.nombre = "X".into();
        assert!(ok(&e, 1, json!({ "cmd": "modificar_entidad", "entidad": serde_json::to_value(&ajena).unwrap() })).is_err());
    }

    #[test]
    fn la_foto_solo_puede_ser_una_que_haya_subido_y_los_adjuntos_nuevos_solo_txt_de_su_carpeta() {
        let e = estado();
        let nueva_foto = ficha_editada(|f| f.foto_url = Some("imagenes/otra.png".into()));
        assert!(ok(&e, 1, nueva_foto.clone()).is_err(), "una imagen que no ha subido");
        let subidas: HashSet<String> = ["imagenes/otra.png".to_string()].into();
        assert!(permitida(&e, 1, &pet(nueva_foto), &subidas).is_ok());

        let con = |ruta: &str| ficha_editada(|f| f.archivos_adjuntos.push(Some(ruta.into())));
        assert!(ok(&e, 1, con("archivos/entidad_10/nuevo.txt")).is_ok());
        assert!(ok(&e, 1, con("archivos/entidad_10/malo.pdf")).is_err(), "PDF prohibido");
        assert!(ok(&e, 1, con("archivos/entidad_10/malo.exe")).is_err());
        assert!(ok(&e, 1, con("archivos/entidad_11/ajeno.txt")).is_err(), "carpeta de otra ficha");
        assert!(ok(&e, 1, con("archivos/entidad_10/sub/dir.txt")).is_err());
        assert!(ok(&e, 1, con("../fuera.txt")).is_err());
    }

    fn adjunto(ruta: &str, id: u16) -> Value {
        json!({ "ruta": ruta, "entidad_archivo": serde_json::to_value(ficha(id, vec![])).unwrap() })
    }

    #[test]
    fn los_archivos_de_texto_son_solo_de_sus_fichas() {
        let e = estado();
        let ruta = "archivos/entidad_10/notas.txt";
        assert!(ok(&e, 1, json!({ "cmd": "leer_archivo_texto", "rutaRelativa": ruta })).is_ok());
        assert!(ok(&e, 1, json!({ "cmd": "leer_archivo_texto", "rutaRelativa": "archivos/entidad_11/notas.txt" })).is_err());
        assert!(ok(&e, 1, json!({ "cmd": "leer_archivo_texto", "rutaRelativa": "imagenes/vieja.png" })).is_err());
        assert!(ok(&e, 1, json!({ "cmd": "guardar_archivo_texto", "adjunto": adjunto(ruta, 10), "contenido": "hola" })).is_ok());
        let enorme = "x".repeat(MAX_TEXTO_JUGADOR + 1);
        assert!(ok(&e, 1, json!({ "cmd": "guardar_archivo_texto", "adjunto": adjunto(ruta, 10), "contenido": enorme })).is_err());
        assert!(ok(&e, 1, json!({ "cmd": "crear_documento_texto", "entidad": serde_json::to_value(ficha(10, vec![])).unwrap() })).is_ok());
        assert!(ok(&e, 1, json!({ "cmd": "crear_documento_texto", "entidad": serde_json::to_value(ficha(11, vec![])).unwrap() })).is_err());
        assert!(ok(&e, 1, json!({ "cmd": "renombrar_archivo_adjunto", "adjunto": adjunto(ruta, 10), "nuevoNombre": "Diario" })).is_ok());
        assert!(ok(&e, 1, json!({ "cmd": "renombrar_archivo_adjunto", "adjunto": adjunto(ruta, 10), "nuevoNombre": "../Diario" })).is_err());
    }

    #[test]
    fn descartar_solo_lo_suyo_y_la_miniatura_solo_de_sus_fichas() {
        let e = estado();
        let subidas: HashSet<String> = ["imagenes/mia.png".to_string()].into();
        let descartar = |rutas: Vec<&str>| pet(json!({ "cmd": "descartar_archivos_borrador", "rutas": rutas }));
        assert!(permitida(&e, 1, &descartar(vec!["imagenes/mia.png", "archivos/entidad_10/x.txt"]), &subidas).is_ok());
        assert!(permitida(&e, 1, &descartar(vec!["imagenes/de_otro.png"]), &subidas).is_err());
        assert!(permitida(&e, 1, &descartar(vec!["archivos/entidad_11/x.txt"]), &subidas).is_err());

        let mini = |id: u16, n: usize| pet(json!({ "cmd": "guardar_miniatura_ficha", "id": id, "datos": vec![0u8; n] }));
        assert!(permitida(&e, 1, &mini(10, 100), &HashSet::new()).is_ok());
        assert!(permitida(&e, 1, &mini(11, 100), &HashSet::new()).is_err());
        assert!(permitida(&e, 1, &mini(10, MAX_MINIATURA + 1), &HashSet::new()).is_err());
    }

    #[test]
    fn ping_y_recorrido_solo_en_mapas_compartidos_y_el_foco_se_acepta_sin_mas() {
        let e = estado();
        let ping = |m: u16| json!({ "cmd": "avisar_tablero", "aviso": { "Ping": { "id_mapa": m, "casilla": { "x": 1, "y": 1 } } } });
        assert!(ok(&e, 1, ping(1)).is_ok());
        assert!(ok(&e, 1, ping(2)).is_err());
        assert!(ok(&e, 1, json!({ "cmd": "avisar_tablero", "aviso": { "RecorridoBorrado": { "id_mapa": 1 } } })).is_ok());
        assert!(ok(&e, 1, json!({ "cmd": "avisar_tablero", "aviso": { "Foco": { "ids_instancias": [3] } } })).is_ok());
    }

    #[test]
    fn el_arrastre_en_vivo_solo_nombra_fichas_suyas_en_un_mapa_compartido() {
        let e = estado();
        let arrastre = |mapa: u16, ids: &[u16]| {
            let piezas: Vec<Value> = ids.iter().map(|id| json!({ "id_instancia": id, "casilla": { "x": 1, "y": 1 } })).collect();
            json!({ "cmd": "avisar_tablero", "aviso": { "Arrastre": { "id_mapa": mapa, "piezas": piezas } } })
        };
        let fin = |mapa: u16, ids: &[u16]| json!({ "cmd": "avisar_tablero", "aviso": { "ArrastreFin": { "id_mapa": mapa, "ids_instancias": ids } } });
        assert!(ok(&e, 1, arrastre(1, &[1])).is_ok());
        assert!(ok(&e, 1, arrastre(1, &[1, 2])).is_err(), "la 2 es del host");
        assert!(ok(&e, 1, arrastre(1, &[3])).is_err(), "la 3 es de otro jugador");
        assert!(ok(&e, 1, arrastre(2, &[1])).is_err(), "mapa sin compartir");
        assert!(ok(&e, 1, fin(1, &[1])).is_ok());
        assert!(ok(&e, 1, fin(1, &[2])).is_err());
        assert!(ok(&e, ID_HOST, arrastre(2, &[1, 2, 3])).is_ok(), "el host no tiene esos límites");
    }

    #[test]
    fn una_ficha_nueva_solo_con_borrador_propio_en_una_zona_que_ve_y_con_topes() {
        let mut e = estado();
        let (subidas, ninguno) = (HashSet::from(["imagenes/mia.png".to_string()]), HashSet::new());
        let suyos = HashSet::from([50u16]);
        let con = |e: &EstadoPartida, v: Value, b: &HashSet<u16>| permitida_con(e, 1, &pet(v), &subidas, b);
        let iniciar = |foto: Value| json!({ "cmd": "iniciar_borrador_entidad", "zonaId": 1, "fotoUrl": foto });

        // Empezar: con un mapa compartido en la zona sí; la foto solo si la subió él
        assert!(con(&e, iniciar(Value::Null), &ninguno).is_ok());
        assert!(con(&e, iniciar(json!("imagenes/mia.png")), &ninguno).is_ok());
        assert!(con(&e, iniciar(json!("imagenes/de_otro.png")), &ninguno).is_err());
        // Tope de borradores abiertos
        assert!(con(&e, iniciar(Value::Null), &HashSet::from([1, 2, 3])).is_err());
        // Tope de fichas propias
        for id in 100..(100 + MAX_FICHAS_PROPIAS as u16) {
            e.banco_entidades.push(ficha(id, vec![1]));
        }
        assert!(con(&e, iniciar(Value::Null), &ninguno).is_err());
        e.banco_entidades.truncate(3);

        // Guardar: solo un borrador suyo, en una zona que ve, con .txt de su carpeta
        let mut nueva = ficha(50, vec![]);
        nueva.foto_url = None;
        nueva.archivos_adjuntos = vec![Some("archivos/entidad_50/a.txt".into())];
        let crear = |n: &Entidad| json!({ "cmd": "crear_entidad", "entidad": serde_json::to_value(n).unwrap() });
        assert!(con(&e, crear(&nueva), &suyos).is_ok());
        assert!(con(&e, crear(&nueva), &ninguno).is_err(), "no es un borrador suyo");
        let mut otra_zona = nueva.clone();
        otra_zona.zona_id = 9;
        assert!(con(&e, crear(&otra_zona), &suyos).is_err());
        let mut pdf = nueva.clone();
        pdf.archivos_adjuntos = vec![Some("archivos/entidad_50/a.pdf".into())];
        assert!(con(&e, crear(&pdf), &suyos).is_err());
        let mut ajeno = nueva.clone();
        ajeno.archivos_adjuntos = vec![Some("archivos/entidad_11/a.txt".into())];
        assert!(con(&e, crear(&ajeno), &suyos).is_err());
        let mut foto = nueva.clone();
        foto.foto_url = Some("imagenes/de_otro.png".into());
        assert!(con(&e, crear(&foto), &suyos).is_err());

        // Los .txt del borrador valen mientras es un borrador, y cancelarlo
        let leer = json!({ "cmd": "leer_archivo_texto", "rutaRelativa": "archivos/entidad_50/a.txt" });
        assert!(con(&e, leer.clone(), &suyos).is_ok());
        assert!(con(&e, leer, &ninguno).is_err());
        let cancelar = |id: u16| json!({ "cmd": "cancelar_borrador_entidad", "id": id, "rutas": ["imagenes/mia.png", "archivos/entidad_50/a.txt"] });
        assert!(con(&e, cancelar(50), &suyos).is_ok());
        assert!(con(&e, cancelar(51), &suyos).is_err());
        // Un mapa sin compartir no abre ninguna zona
        e.mapas.iter_mut().for_each(|m| m.compartido = false);
        assert!(con(&e, iniciar(Value::Null), &ninguno).is_err());
    }
}
