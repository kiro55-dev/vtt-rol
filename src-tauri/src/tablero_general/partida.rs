// src-tauri/src/tablero_general/partida.rs
//
// Comandos de Tauri y estado compartido. La lógica de estado/deltas está en `estado.rs`
// y el acceso a disco en `almacen.rs`.
use std::collections::HashSet;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::red::{self, Red};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::tablero_general::almacen::{bloquear, Almacen};
use crate::tablero_general::archivos;
use crate::tablero_general::cambios_entidad;
use crate::tablero_general::estado::{self, DeltaPartida, EstadoPartida};
use crate::tablero_general::imagenes;
use crate::tablero_general::instancias;
use crate::tablero_general::jugadores;
use crate::tablero_general::miniaturas::{self, Tipo};
use crate::tablero_general::modelos::*;
use crate::tablero_general::papelera;
use crate::tablero_general::partidas;
use crate::tablero_general::recursos;
use crate::tablero_general::tablero::{self, AvisoTablero};
use crate::tablero_general::visibilidad;

pub struct AppState {
    /// En `Arc` para que el servidor de la sala (red/) también pueda leer y registrar cambios.
    pub estado: Arc<Mutex<EstadoPartida>>,
    pub almacen: Arc<Almacen>,
    /// Ids reservados por fichas NUEVAS que se están editando y todavía no están en el banco.
    /// Orden de bloqueo: siempre `estado` primero y `borradores` (o la red) después.
    borradores: Mutex<HashSet<u16>>,
    /// Carpeta de datos de la app: las partidas están en `<raiz>/partidas/`.
    pub raiz: PathBuf,
    /// La sala de la partida abierta (ver red/).
    pub red: Arc<Red>,
}

impl AppState {
    pub fn nuevo(estado: EstadoPartida, almacen: Almacen, raiz: PathBuf) -> Self {
        let estado = Arc::new(Mutex::new(estado));
        let almacen = Arc::new(almacen);
        let red = Arc::new(Red::nueva(estado.clone(), almacen.clone(), raiz.clone()));
        Self { estado, almacen, borradores: Mutex::new(HashSet::new()), raiz, red }
    }

    pub fn lock(&self) -> MutexGuard<'_, EstadoPartida> {
        bloquear(&self.estado)
    }

    fn reservas(&self) -> MutexGuard<'_, HashSet<u16>> {
        bloquear(&self.borradores)
    }
}

// ========================================================
// DESPACHO DE CAMBIOS
// ========================================================

/// Registra un delta con el lock YA cogido (para comandos que leen y escriben a la vez).
///
/// Si el cambio puede dejar sin uso imágenes o archivos (borrar un mapa o una ficha, cambiar una
/// foto, quitar un adjunto...), se comparan las referencias de antes y de después y se borra lo que
/// sobra, pero solo cuando el delta ya está escrito en el log: si algo falla, el cambio no se da
/// por hecho y no se borra nada.
///
/// Con la sala abierta, cada jugador conectado recibe lo que cambió en SU vista (red::tras_cambio), todavía
/// bajo el mismo lock: el orden de lo que reciben es el del log.
pub(crate) fn registrar(est: &mut EstadoPartida, almacen: &Almacen, delta: DeltaPartida) {
    let antes = recursos::afecta_a_recursos(&delta).then(|| recursos::referencias(est));

    match est.registrar(almacen, delta) {
        Ok(()) => {
            if let Some(antes) = antes {
                let liberadas = recursos::liberar(&almacen.dir(), &antes, &recursos::referencias(est));
                if !liberadas.is_empty() {
                    eprintln!("[recursos] liberados: {liberadas:?}");
                }
            }
        }
        Err(e) => eprintln!("[partida] {e}"),
    }
    red::tras_cambio(est);
}

/// Punto de entrada para cualquier cambio: lo aplica en RAM y lo añade al log, en ese orden
/// y bajo el mismo lock, así el orden del log siempre coincide con el del estado.
pub fn procesar_delta(state: &AppState, delta: DeltaPartida) {
    let mut est = state.lock();
    registrar(&mut est, &state.almacen, delta);
}

// Función específica para registrar tiradas (llamada por dados.rs).
// Recibe el desglose y el total por separado: releer el total de un texto con `parse().unwrap_or(0)`
// guardaba un 0 silencioso cuando el número no cabía.
// `jugador` es quien tiró (`ID_HOST` o un jugador de la sala): su nombre se guarda en la tirada para el chat.
pub fn registrar_tirada(state: &AppState, jugador: IdJugador, desglose: &str, total: i32) {
    let mut est = state.lock();
    let autor = est.jugadores.iter().find(|j| j.id == jugador).map(|j| j.nombre.clone()).unwrap_or_default();
    let tirada = Tiradas {
        total,
        comando: desglose.to_string(),
        hora: chrono::Local::now().format("%H:%M").to_string(),
        jugador,
        autor,
    };

    registrar(&mut est, &state.almacen, DeltaPartida::TiradaRealizada(tirada));
}

// Historial personal de comandos (dados.rs)
pub fn registrar_comando_personal(state: &AppState, comando: &str) {
    state.almacen.anadir_historial(comando);
}

// ========================================================
// GUARDADO
// ========================================================

/// Vuelca la RAM a partida.json (atómico) y vacía el log. Mantiene el lock durante la
/// operación a propósito: si se soltara antes de vaciar el log, un delta llegado entre
/// medias se perdería.
pub fn compactar_partida(state: &AppState) {
    let est = state.lock();
    if let Err(e) = estado::compactar(&state.almacen, &est) {
        eprintln!("[partida] {e}");
    }
}

// BUCLE DE AUTOGUARDADO CADA 10 MINUTOS
pub fn iniciar_autoguardado(app_handle: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut intervalo = tokio::time::interval(std::time::Duration::from_secs(600));
        intervalo.tick().await; // el primer tick es inmediato: lo descartamos

        loop {
            intervalo.tick().await;
            if let Some(state) = app_handle.try_state::<AppState>() {
                compactar_partida(&state);
            }
        }
    });
}

// ========================================================
// PARTIDAS (una carpeta cada una)
// ========================================================

/// Nombre de la partida abierta: la última parte de la ruta de su carpeta.
fn nombre_partida_actual(state: &AppState) -> String {
    state
        .almacen
        .dir()
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Guarda la partida abierta y pasa a la otra. Con el lock del estado cogido todo el tiempo (igual
/// que `compactar_partida`): ningún comando puede cambiar nada a mitad, y el log nunca recibe un
/// delta de una partida en la carpeta de otra. Si algo falla, la partida abierta sigue como estaba.
fn cambiar_de_partida(state: &AppState, nombre: &str) -> Result<(), String> {
    let mut est = state.lock();
    let reservas = state.reservas();
    // Orden de bloqueo: `estado` y luego `borradores`. Una ficha nueva abierta tiene su id reservado
    // en ESTA partida y su carpeta de archivos en ella: no puede acompañarnos a la otra.
    if !reservas.is_empty() {
        return Err("Cierra la ficha que tienes abierta antes de cambiar de partida".into());
    }
    // Los jugadores conectados están en ESTA partida (cada partida es un mundo aparte, con su sala)
    if state.red.esta_abierta() {
        return Err("Cierra la sala antes de cambiar de partida".into());
    }

    // `buscar` solo devuelve carpetas que existen, así que el nombre nunca sale de `partidas/`
    let nombre = partidas::buscar(&state.raiz, nombre).ok_or("Esa partida no existe")?;
    let anterior = state.almacen.dir();
    let nueva = partidas::ruta_partida(&state.raiz, &nombre);
    if nueva == anterior {
        return Ok(());
    }

    // 1. Se guarda la que se deja (snapshot atómico + log vacío) ANTES de tocar nada. Se sale de ella
    //    de forma normal, así que su papelera se vacía (si no, sus archivos se quedarían para siempre).
    vaciar_papelera(&mut est, &state.almacen);
    estado::compactar(&state.almacen, &est).map_err(|e| format!("No se pudo guardar la partida actual: {e}"))?;

    // 2. Se abre la nueva; si no se puede leer, se vuelve a la anterior (intacta en memoria)
    state.almacen.cambiar_dir(&nueva).map_err(|e| format!("No se pudo abrir la partida: {e}"))?;
    match estado::cargar(&state.almacen) {
        Ok((mut nuevo, informe)) => {
            eprintln!("[partida] abierta «{nombre}» · {informe:?}");
            migrar_fondos_antiguos(&mut nuevo, &state.almacen);
            corregir_tamanos_mapas(&mut nuevo, &state.almacen);
            if informe.conviene_compactar() {
                if let Err(e) = estado::compactar(&state.almacen, &nuevo) {
                    eprintln!("[partida] {e}");
                }
            }
            *est = nuevo;
            partidas::guardar_ultima(&state.raiz, &nombre);
            Ok(())
        }
        Err(e) => {
            if let Err(e2) = state.almacen.cambiar_dir(&anterior) {
                eprintln!("[partida] no se pudo volver a la partida anterior: {e2}");
            }
            Err(e)
        }
    }
}

#[derive(Serialize)]
pub struct InfoPartidas {
    pub actual: String,
    pub partidas: Vec<String>,
}

#[tauri::command]
pub fn obtener_partidas(state: State<'_, AppState>) -> InfoPartidas {
    InfoPartidas { actual: nombre_partida_actual(&state), partidas: partidas::listar(&state.raiz) }
}

/// Guarda la partida abierta y abre otra.
#[tauri::command]
pub async fn abrir_partida(nombre: String, state: State<'_, AppState>) -> Result<(), String> {
    cambiar_de_partida(&state, &nombre)
}

/// Crea una partida vacía (con los parámetros de serie) y pasa a ella, guardando antes la actual.
#[tauri::command]
pub async fn crear_partida(nombre: String, state: State<'_, AppState>) -> Result<(), String> {
    let nombre = partidas::crear(&state.raiz, &nombre)?;
    cambiar_de_partida(&state, &nombre)
}

// Comando opcional por si se implementa botón de guardar partida
#[tauri::command]
pub async fn _guardar_partida(state: State<'_, AppState>) -> Result<(), String> {
    compactar_partida(&state);
    Ok(())
}

// ========================================================
// COMANDOS DE LECTURA (leen de RAM)
// ========================================================

#[tauri::command]
pub fn obtener_historial_tiradas(state: State<'_, AppState>) -> Vec<Tiradas> {
    state.lock().historial_tiradas.clone()
}

#[tauri::command]
pub fn obtener_definiciones_parametros(state: State<'_, AppState>) -> Vec<DefinicionParametro> {
    state.lock().definiciones_parametros.clone()
}

const MAX_PARAMETROS: usize = 40;

/// Comprueba una definición NUEVA contra las que ya existen.
fn validar_definicion(existentes: &[DefinicionParametro], d: &DefinicionParametro) -> Result<(), String> {
    let clave = d.clave.trim();
    if clave.is_empty() {
        return Err("El parámetro necesita un nombre".into());
    }
    if clave.chars().count() > 30 {
        return Err("El nombre del parámetro no puede pasar de 30 caracteres".into());
    }
    if existentes.iter().any(|e| e.clave.trim().to_lowercase() == clave.to_lowercase()) {
        return Err(format!("Ya existe un parámetro llamado «{clave}»"));
    }
    if existentes.len() >= MAX_PARAMETROS {
        return Err(format!("No puede haber más de {MAX_PARAMETROS} parámetros"));
    }
    if d.tiene_maximo && !matches!(d.valor_por_defecto, ValorParametro::Numero(_)) {
        return Err("Solo un número puede tener máximo".into());
    }
    if matches!(d.valor_por_defecto, ValorParametro::Numero(n) if n < 0) {
        return Err("El valor por defecto no puede ser negativo".into());
    }
    if d.unidad.as_deref().is_some_and(|u| u.chars().count() > 8) {
        return Err("La unidad no puede pasar de 8 caracteres".into());
    }
    if let Some(Forma::Rombo(c) | Forma::Circulo(c) | Forma::Cuadrado(c) | Forma::BarraVida(c) | Forma::Resumen(c)) =
        &d.representar_mapa
    {
        if c.len() != 4 {
            return Err("El color de la forma no es válido".into());
        }
        if !matches!(d.valor_por_defecto, ValorParametro::Numero(_)) {
            return Err("Solo un número puede dibujarse en el tablero".into());
        }
    }
    if matches!(d.representar_mapa, Some(Forma::Resumen(_))) && !d.tiene_maximo {
        return Err("El resumen «actual/máx» solo vale para parámetros con máximo".into());
    }
    if d.atenuar_si_negativo && !d.tiene_maximo {
        return Err("Atenuar la ficha solo vale para parámetros con máximo".into());
    }
    Ok(())
}

/// Crea un parámetro común a todas las fichas (el id lo pone Rust). Cada ficha lo recibe con su valor por defecto.
#[tauri::command]
pub fn crear_definicion_parametro(
    definicion: DefinicionParametro,
    state: State<'_, AppState>,
) -> Result<DefinicionParametro, String> {
    let mut est = state.lock();
    validar_definicion(&est.definiciones_parametros, &definicion)?;

    // Un id nunca usado por ninguna definición actual (los de las borradas ya no están en ninguna ficha)
    let id = est.definiciones_parametros.iter().map(|d| d.id).max().unwrap_or(0) + 1;
    let nueva = DefinicionParametro {
        id,
        clave: definicion.clave.trim().to_string(),
        unidad: definicion.unidad.map(|u| u.trim().to_string()).filter(|u| !u.is_empty()),
        ..definicion
    };
    registrar(&mut est, &state.almacen, DeltaPartida::ParametroCreado(nueva.clone()));
    Ok(nueva)
}

/// Cambia la configuración de un parámetro (nombre, máximo, unidad, forma, visibilidad, valor por
/// defecto). El tipo no se puede cambiar: los valores que ya tienen las fichas dejarían de valer.
#[tauri::command]
pub fn modificar_definicion_parametro(
    definicion: DefinicionParametro,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut est = state.lock();
    let actual = est
        .definiciones_parametros
        .iter()
        .find(|d| d.id == definicion.id)
        .ok_or("El parámetro ya no existe")?;
    if !actual.valor_por_defecto.mismo_tipo(&definicion.valor_por_defecto) {
        return Err("No se puede cambiar el tipo de un parámetro; créalo de nuevo".into());
    }

    let otras: Vec<DefinicionParametro> =
        est.definiciones_parametros.iter().filter(|d| d.id != definicion.id).cloned().collect();
    validar_definicion(&otras, &definicion)?;

    let nueva = DefinicionParametro {
        clave: definicion.clave.trim().to_string(),
        unidad: definicion.unidad.clone().map(|u| u.trim().to_string()).filter(|u| !u.is_empty()),
        ..definicion
    };
    registrar(&mut est, &state.almacen, DeltaPartida::ParametroModificado(nueva));
    Ok(())
}

/// Borra un parámetro: desaparece de la lista y de todas las fichas (y sus instancias).
#[tauri::command]
pub fn borrar_definicion_parametro(id: u16, state: State<'_, AppState>) -> Result<(), String> {
    let mut est = state.lock();
    if !est.definiciones_parametros.iter().any(|d| d.id == id) {
        return Err("El parámetro ya no existe".into());
    }
    registrar(&mut est, &state.almacen, DeltaPartida::ParametroBorrado(id));
    Ok(())
}

/// Cambia el orden de los parámetros (que es el que siguen las fichas). `ids` debe tener todos, una vez.
#[tauri::command]
pub fn reordenar_definiciones_parametros(ids: Vec<u16>, state: State<'_, AppState>) -> Result<(), String> {
    let mut est = state.lock();
    let mut actuales: Vec<u16> = est.definiciones_parametros.iter().map(|d| d.id).collect();
    let mut pedidos = ids.clone();
    actuales.sort_unstable();
    pedidos.sort_unstable();
    if actuales != pedidos {
        return Err("La lista de parámetros ha cambiado; vuelve a intentarlo".into());
    }
    registrar(&mut est, &state.almacen, DeltaPartida::ParametrosReordenados(ids));
    Ok(())
}

#[tauri::command]
pub fn obtener_zonas(state: State<'_, AppState>) -> Vec<Zona> {
    state.lock().zonas.clone()
}

#[tauri::command]
pub fn obtener_mapas_zona(zona_id: u16, state: State<'_, AppState>) -> Vec<Mapa> {
    state.lock().mapas.iter().filter(|m| m.id_zona == zona_id).cloned().collect()
}

#[tauri::command]
pub fn obtener_banco_entidades(state: State<'_, AppState>) -> Vec<Entidad> {
    state.lock().banco_entidades.clone()
}

/// Las instancias tal como las muestra la pantalla del máster: enteras (ver
/// `visibilidad::OCULTAR_PRIVADOS_AL_MASTER`); lo público solo decide lo que se envía a los jugadores.
#[tauri::command]
pub fn obtener_instancias_activas(state: State<'_, AppState>) -> Vec<Instancia> {
    let est = state.lock();
    visibilidad::instancias_para_el_master(&est.instancias_activas, visibilidad::OCULTAR_PRIVADOS_AL_MASTER, &est.definiciones_parametros)
}

// Instancias: las reglas (ficha única, casilla encajada, id nuevo...) están en `instancias.rs`;
// aquí solo se registra el delta que devuelven.

/// Instancia una ficha del banco en una casilla (`destino`) o solo como activa (`None`).
/// Una ficha única que ya tenía instancia la reutiliza. Devuelve la instancia resultante.
#[tauri::command]
pub fn instanciar_entidad(id_entidad: u16,
    destino: Option<instancias::Destino>, state: State<'_, AppState>) -> Result<Instancia, String> {
    instanciar_entidad_en(id_entidad, destino, &state)
}

/// Lo que hace `instanciar_entidad`, con el estado ya en la mano (así también lo ejecuta el host a petición de un jugador).
pub fn instanciar_entidad_en(id_entidad: u16,
    destino: Option<instancias::Destino>, state: &AppState) -> Result<Instancia, String> {
    let mut est = state.lock();
    let delta = instancias::instanciar(&est, id_entidad, destino)?;
    let (DeltaPartida::InstanciaCreada(i) | DeltaPartida::InstanciaModificada(i)) = &delta else {
        unreachable!("instanciar solo crea o modifica")
    };
    let instancia = i.clone();
    registrar(&mut est, &state.almacen, delta);
    // Lo que vuelve a la pantalla del máster, igual que `obtener_instancias_activas`
    Ok(visibilidad::instancias_para_el_master(&[instancia], visibilidad::OCULTAR_PRIVADOS_AL_MASTER, &est.definiciones_parametros).remove(0))
}

/// Instancia varias fichas del banco de golpe (selección múltiple). Con `destino`, la primera cae en esa
/// casilla y las demás a su derecha y debajo, en cuadrícula (`tablero::disposicion_en_cuadricula`); sin
/// él, todas quedan activas sin colocar. Cada una es un delta normal, registrado en orden bajo el mismo lock.
#[tauri::command]
pub fn instanciar_entidades(ids: Vec<u16>,
    destino: Option<instancias::Destino>, state: State<'_, AppState>) -> Result<Vec<Instancia>, String> {
    instanciar_entidades_en(ids, destino, &state)
}

/// Lo que hace `instanciar_entidades`, con el estado ya en la mano (así también lo ejecuta el host a petición de un jugador).
pub fn instanciar_entidades_en(ids: Vec<u16>,
    destino: Option<instancias::Destino>, state: &AppState) -> Result<Vec<Instancia>, String> {
    let mut est = state.lock();
    let mut destinos: Vec<Option<instancias::Destino>> = vec![None; ids.len()];
    if let Some(d) = destino {
        let mapa = est.mapas.iter().find(|m| m.id == d.id_mapa).ok_or("Ese mapa no existe")?;
        let lados: Vec<(u16, u16)> = ids
            .iter()
            .map(|id| {
                let e = est.banco_entidades.iter().find(|e| e.id == *id).ok_or("Esa ficha no existe")?;
                // Una única que ya está en el tablero conserva su giro al moverse
                let reutilizada = e.es_unico.then(|| est.instancias_activas.iter().find(|i| i.base.id == e.id)).flatten();
                Ok(reutilizada.map_or_else(|| tablero::tamano_cuerpo(&e.cuerpo), tablero::lados_instancia))
            })
            .collect::<Result<_, String>>()?;
        let casillas = tablero::disposicion_en_cuadricula(tablero::Rejilla::de(mapa), d.casilla, &lados);
        destinos = casillas.into_iter().map(|casilla| Some(instancias::Destino { id_mapa: d.id_mapa, casilla })).collect();
    }

    let mut creadas = Vec::new();
    for (id, destino) in ids.into_iter().zip(destinos) {
        let delta = instancias::instanciar(&est, id, destino)?;
        if let DeltaPartida::InstanciaCreada(i) | DeltaPartida::InstanciaModificada(i) = &delta {
            creadas.push(i.clone());
        }
        registrar(&mut est, &state.almacen, delta);
    }
    Ok(visibilidad::instancias_para_el_master(&creadas, visibilidad::OCULTAR_PRIVADOS_AL_MASTER, &est.definiciones_parametros))
}

/// Mueve una instancia a otra casilla o, con `None`, la quita del tablero (sigue activa).
#[tauri::command]
pub fn mover_instancia(id_instancia: u16, destino: Option<instancias::Destino>, state: State<'_, AppState>) -> Result<(), String> {
    mover_instancia_en(id_instancia, destino, &state)
}

/// Lo que hace `mover_instancia`, con el estado ya en la mano (así también lo ejecuta el host a petición de un jugador).
pub fn mover_instancia_en(id_instancia: u16, destino: Option<instancias::Destino>, state: &AppState) -> Result<(), String> {
    let mut est = state.lock();
    let delta = instancias::mover(&est, id_instancia, destino)?;
    registrar(&mut est, &state.almacen, delta);
    Ok(())
}

/// Mueve varias instancias del mismo mapa a la vez `(dx, dy)` casillas, conservando sus distancias
/// (las seleccionadas que se arrastran juntas). Un `InstanciaModificada` por ficha, en orden y bajo el mismo lock.
#[tauri::command]
pub fn mover_instancias(ids: Vec<u16>, id_mapa: u16, dx: i32, dy: i32, state: State<'_, AppState>) -> Result<(), String> {
    mover_instancias_en(ids, id_mapa, dx, dy, &state)
}

/// Lo que hace `mover_instancias`, con el estado ya en la mano (así también lo ejecuta el host a petición de un jugador).
pub fn mover_instancias_en(ids: Vec<u16>, id_mapa: u16, dx: i32, dy: i32, state: &AppState) -> Result<(), String> {
    let mut est = state.lock();
    for delta in instancias::mover_grupo(&est, &ids, id_mapa, dx, dy)? {
        registrar(&mut est, &state.almacen, delta);
    }
    Ok(())
}

/// Gira la instancia un cuarto de vuelta en el sentido del reloj.
#[tauri::command]
pub fn rotar_instancia(id_instancia: u16, state: State<'_, AppState>) -> Result<(), String> {
    rotar_instancia_en(id_instancia, &state)
}

/// Lo que hace `rotar_instancia`, con el estado ya en la mano (así también lo ejecuta el host a petición de un jugador).
pub fn rotar_instancia_en(id_instancia: u16, state: &AppState) -> Result<(), String> {
    let mut est = state.lock();
    let delta = instancias::rotar(&est, id_instancia)?;
    registrar(&mut est, &state.almacen, delta);
    Ok(())
}

/// Pone o quita el espejo de la imagen de la instancia (solo aspecto).
#[tauri::command]
pub fn espejar_instancia(id_instancia: u16, state: State<'_, AppState>) -> Result<(), String> {
    espejar_instancia_en(id_instancia, &state)
}

/// Lo que hace `espejar_instancia`, con el estado ya en la mano (así también lo ejecuta el host a petición de un jugador).
pub fn espejar_instancia_en(id_instancia: u16, state: &AppState) -> Result<(), String> {
    let mut est = state.lock();
    let delta = instancias::espejar(&est, id_instancia)?;
    registrar(&mut est, &state.almacen, delta);
    Ok(())
}

/// Nuevo orden de «Fichas activas» (todas las instancias, de arriba abajo).
#[tauri::command]
pub fn reordenar_instancias(ids: Vec<u16>, state: State<'_, AppState>) -> Result<(), String> {
    let mut est = state.lock();
    let delta = instancias::reordenar(&est, ids)?;
    registrar(&mut est, &state.almacen, delta);
    Ok(())
}

/// La instancia deja de estar activa.
#[tauri::command]
pub fn borrar_instancia(id_instancia: u16, state: State<'_, AppState>) -> Result<(), String> {
    borrar_instancia_en(id_instancia, &state)
}

/// Lo que hace `borrar_instancia`, con el estado ya en la mano (así también lo ejecuta el host a petición de un jugador).
pub fn borrar_instancia_en(id_instancia: u16, state: &AppState) -> Result<(), String> {
    let mut est = state.lock();
    let delta = instancias::borrar(&est, id_instancia)?;
    registrar(&mut est, &state.almacen, delta);
    Ok(())
}

/// Suma `delta` al valor actual (p. ej. la vida) de una instancia, sin bajar de 0 ni pasar del máximo.
#[tauri::command]
pub fn modificar_valor_actual(id_instancia: u16,
    id_definicion: u16,
    delta: i16, state: State<'_, AppState>) -> Result<(), String> {
    modificar_valor_actual_en(id_instancia, id_definicion, delta, &state)
}

/// Lo que hace `modificar_valor_actual`, con el estado ya en la mano (así también lo ejecuta el host a petición de un jugador).
pub fn modificar_valor_actual_en(id_instancia: u16,
    id_definicion: u16,
    delta: i16, state: &AppState) -> Result<(), String> {
    let mut est = state.lock();
    if let Some(delta) = instancias::cambiar_valor_actual(&est, id_instancia, id_definicion, delta)? {
        registrar(&mut est, &state.almacen, delta);
    }
    Ok(())
}

#[tauri::command]
pub fn obtener_historial_personal(state: State<'_, AppState>) -> Vec<String> {
    state.almacen.leer_historial()
}

// ========================================================
// ZONAS Y MAPAS
// ========================================================

#[tauri::command]
pub fn crear_zona(state: State<'_, AppState>) -> Zona {
    // Calcular el id y registrar el delta en la MISMA sección crítica: dos llamadas
    // simultáneas ya no pueden obtener el mismo id.
    let mut est = state.lock();

    // Nunca el de una zona que está en la papelera: al restaurarla tiene que estar libre
    let id = est.zonas.iter().map(|z| z.id).max().unwrap_or(0).max(papelera::reservados(&est).zona) + 1;
    let orden = est.zonas.iter().map(|z| z.orden).max().map(|o| o + 1).unwrap_or(0);

    let nueva_zona = Zona {
        id,
        nombre: format!("Zona {}", id),
        orden,
    };

    registrar(&mut est, &state.almacen, DeltaPartida::ZonaCreada(nueva_zona.clone()));
    nueva_zona
}

/// Manda la zona a la papelera con sus mapas y sus fichas (se puede restaurar con Ctrl+Z).
#[tauri::command]
pub fn eliminar_zona(zona: Zona, state: State<'_, AppState>) -> Result<(), String> {
    let mut est = state.lock();
    let delta = papelera::borrar_zona(&est, zona.id)?;
    registrar(&mut est, &state.almacen, delta);
    Ok(())
}

#[tauri::command]
pub fn modificar_zona(zona: Zona, state: State<'_, AppState>) {
    procesar_delta(&state, DeltaPartida::ZonaModificada(zona));
}

/// `url_imagen_fondo` es la ruta RELATIVA devuelta por `importar_imagen`
/// (p. ej. "imagenes/ab12...png"), no una URL ni una ruta del sistema.
#[tauri::command]
pub fn crear_mapa(
    id_zona: u16,
    url_imagen_fondo: Option<String>,
    state: State<'_, AppState>,
) -> Result<Mapa, String> {
    // Solo lee la cabecera de la imagen: se hace antes de coger el lock
    let (ancho, alto) = tamano_de_fondo(&state.almacen.dir(), url_imagen_fondo.as_deref());
    let mut est = state.lock();

    let zona = est
        .zonas
        .iter()
        .find(|z| z.id == id_zona)
        .cloned()
        .ok_or_else(|| format!("La zona {} no existe", id_zona))?;

    // ID secuencial global para evitar cualquier colisión entre zonas
    let id = est.mapas.iter().map(|m| m.id).max().unwrap_or(0).max(papelera::reservados(&est).mapa) + 1;
    let orden = est
        .mapas
        .iter()
        .filter(|m| m.id_zona == id_zona)
        .map(|m| m.orden)
        .max()
        .map(|o| o + 1)
        .unwrap_or(0);

    let nuevo_mapa = Mapa {
        id,
        id_zona,
        nombre: id.to_string(), // Nombre por defecto es su propio ID
        url_imagen_fondo,
        ancho,
        alto,
        rejilla: 50,
        color_opacidad: vec![255, 255, 255, 76],
        grosor: 1,
        orden,
        compartido: false,
    };

    registrar(&mut est, &state.almacen, DeltaPartida::MapaCreado(zona, nuevo_mapa.clone()));
    Ok(nuevo_mapa)
}

/// El tamaño del mapa (`ancho`/`alto`) no lo decide el front: es el de su imagen de fondo y lo
/// calcula Rust. Si cambia el fondo se vuelve a leer; si no, se conserva el guardado. Tampoco decide si
/// está compartido (eso es `compartir_mapa`): una copia vieja del front no puede dejar de compartirlo.
#[tauri::command]
pub fn modificar_mapa(mut mapa: Mapa, state: State<'_, AppState>) -> Result<(), String> {
    let dir = state.almacen.dir();
    let mut est = state.lock();
    let guardado = est.mapas.iter().find(|m| m.id == mapa.id).ok_or("Ese mapa no existe")?;

    mapa.compartido = guardado.compartido;
    (mapa.ancho, mapa.alto) = if guardado.url_imagen_fondo == mapa.url_imagen_fondo {
        (guardado.ancho, guardado.alto)
    } else {
        tamano_de_fondo(&dir, mapa.url_imagen_fondo.as_deref())
    };
    tablero::validar_mapa(&mapa)?;

    registrar(&mut est, &state.almacen, DeltaPartida::MapaModificado(mapa));
    Ok(())
}

/// Tamaño de un mapa sin imagen, o con una que no se puede leer.
const TAMANO_MAPA_DEFECTO: (u16, u16) = (1000, 1000);

/// Tamaño en px que debe tener un mapa con este fondo.
fn tamano_de_fondo(dir: &Path, url_imagen_fondo: Option<&str>) -> (u16, u16) {
    url_imagen_fondo.and_then(|u| imagenes::dimensiones(dir, u)).unwrap_or(TAMANO_MAPA_DEFECTO)
}

/// Pone a cada mapa con fondo el tamaño real de su imagen (los mapas antiguos se crearon todos
/// con 1000×1000). Se llama al abrir una partida, ANTES de compactar. Cada corrección es un
/// `MapaModificado` normal, así que queda en el log como cualquier otro cambio.
pub fn corregir_tamanos_mapas(est: &mut EstadoPartida, almacen: &Almacen) {
    let dir = almacen.dir();
    let corregidos: Vec<Mapa> = est
        .mapas
        .iter()
        .filter_map(|m| {
            let (ancho, alto) = imagenes::dimensiones(&dir, m.url_imagen_fondo.as_deref()?)?;
            (m.ancho != ancho || m.alto != alto).then(|| Mapa { ancho, alto, ..m.clone() })
        })
        .collect();
    for mapa in corregidos {
        eprintln!("[partida] mapa {} pasa a medir {}×{} px", mapa.id, mapa.ancho, mapa.alto);
        registrar(est, almacen, DeltaPartida::MapaModificado(mapa));
    }
}

/// El archivo del disco al que apunta una URL de versiones antiguas de la app (`http://asset.localhost/<ruta
/// codificada>` en Windows, `asset://localhost/<ruta>` en otros sistemas). `None` si no es una de ellas.
fn ruta_de_url_antigua(url: &str) -> Option<PathBuf> {
    let resto = ["http://asset.localhost/", "https://asset.localhost/", "asset://localhost/"]
        .iter()
        .find_map(|p| url.strip_prefix(p))?;
    let resto = resto.split(['?', '#']).next().unwrap_or("");
    let bytes = resto.as_bytes();
    let mut salida = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            salida.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            salida.push(bytes[i]);
            i += 1;
        }
    }
    let ruta = String::from_utf8(salida).ok()?;
    (!ruta.is_empty()).then(|| PathBuf::from(ruta))
}

/// Las imágenes de versiones antiguas se guardaban como URL de un archivo del disco del host (fuera de la carpeta
/// de datos): el host las veía, pero no se podían medir, ni proteger, ni servir a un jugador. Aquí se copian a
/// `imagenes/` y la partida pasa a usar la ruta nueva (un delta normal por cada una). Si el archivo ya no existe
/// se deja como estaba. Se llama al abrir una partida, ANTES de `corregir_tamanos_mapas` (que mide el mapa nuevo).
pub fn migrar_fondos_antiguos(est: &mut EstadoPartida, almacen: &Almacen) {
    let dir = almacen.dir();
    let migrar = |url: &str, uso: imagenes::Uso| -> Option<String> {
        let origen = ruta_de_url_antigua(url)?;
        match imagenes::importar(&dir, &origen, uso) {
            Ok(nueva) => Some(nueva),
            Err(e) => {
                eprintln!("[partida] no se pudo migrar {}: {e}", origen.display());
                None
            }
        }
    };
    let mapas: Vec<Mapa> = est
        .mapas
        .iter()
        .filter_map(|m| {
            let nueva = migrar(m.url_imagen_fondo.as_deref()?, imagenes::Uso::Mapa)?;
            Some(Mapa { url_imagen_fondo: Some(nueva), ..m.clone() })
        })
        .collect();
    for mapa in mapas {
        eprintln!("[partida] el fondo del mapa {} pasa a imagenes/", mapa.id);
        registrar(est, almacen, DeltaPartida::MapaModificado(mapa));
    }
    let fotos: Vec<(u16, String)> = est
        .banco_entidades
        .iter()
        .filter_map(|e| Some((e.id, migrar(e.foto_url.as_deref()?, imagenes::Uso::Ficha)?)))
        .collect();
    for (id, nueva) in fotos {
        eprintln!("[partida] la foto de la ficha {id} pasa a imagenes/");
        let cambios = vec![cambios_entidad::CambioEntidad::Foto(Some(nueva))];
        registrar(est, almacen, DeltaPartida::EntidadCambiada { id, cambios });
    }
}

/// El host abre un mapa (o ninguno). Los jugadores no eligen mapa: ven el que el host tiene abierto si lo
/// comparte, y si entra en uno que no se comparte no notan nada (ver `visibilidad::mapa_visible`). Es efímero: no
/// cambia la partida, no se guarda ni va al log.
#[tauri::command]
pub fn abrir_mapa(id: Option<u16>, state: State<'_, AppState>) {
    abrir_mapa_en(id, &state)
}

pub fn abrir_mapa_en(id: Option<u16>, state: &AppState) {
    let mut est = state.lock();
    est.mapa_del_host.abierto = id;
    if let Some(compartido) = id.filter(|i| est.mapas.iter().any(|m| m.id == *i && m.compartido)) {
        est.mapa_del_host.ultimo_publico = Some(compartido);
    }
    state.red.tras_cambio(&est);
}

/// La casilla de un mapa: lo ven (o dejan de ver) los jugadores de la sala, al instante. Es independiente
/// del mapa que tenga abierto el host, así puede preparar otros sin que se vean.
#[tauri::command]
pub fn compartir_mapa(id: u16, compartido: bool, state: State<'_, AppState>) -> Result<(), String> {
    {
        let mut est = state.lock();
        let mapa = est.mapas.iter().find(|m| m.id == id).ok_or("Ese mapa no existe")?;
        if mapa.compartido == compartido {
            return Ok(());
        }
        // Si es el que el host tiene abierto, pasa a ser también el último que ven los jugadores
        if compartido && est.mapa_del_host.abierto == Some(id) {
            est.mapa_del_host.ultimo_publico = Some(id);
        }
        registrar(&mut est, &state.almacen, DeltaPartida::MapaCompartido { id, compartido });
    }
    state.red.avisar(); // la ventana de la sala lista los mapas compartidos
    Ok(())
}

/// Ids de los mapas que ven los jugadores.
pub fn mapas_compartidos(est: &EstadoPartida) -> Vec<u16> {
    est.mapas.iter().filter(|m| m.compartido).map(|m| m.id).collect()
}

/// Manda el mapa a la papelera con las instancias que tenía (se puede restaurar con Ctrl+Z).
#[tauri::command]
pub fn borrar_mapa(mapa: Mapa, state: State<'_, AppState>) -> Result<(), String> {
    let mut est = state.lock();
    let delta = papelera::borrar_mapa(&est, mapa.id)?;
    registrar(&mut est, &state.almacen, delta);
    Ok(())
}

// ========================================================
// AVISOS DEL TABLERO (efímeros)
// ========================================================

/// El host ha hecho un ping o ha movido su recorrido en el tablero (ver `AvisoTablero`). Un aviso no es un
/// delta (no se aplica al estado ni se guarda): su propio tablero ya lo ha dibujado, y esto lo envía a los
/// jugadores si es de un mapa que ven (`Red::difundir`). FUTURA RED: al RECIBIR uno de un jugador, se
/// emitirá al front el evento de Tauri "aviso-tablero" con el mismo contenido (src/lib/avisosTablero.ts).
#[tauri::command]
pub fn avisar_tablero(aviso: AvisoTablero, state: State<'_, AppState>) -> Result<(), String> {
    let compartidos = {
        let est = state.lock();
        match aviso.id_mapa() {
            Some(id) => aviso.validar(est.mapas.iter().find(|m| m.id == id).ok_or("Ese mapa no existe")?)?,
            None => {
                let ids: Vec<u16> = est.instancias_activas.iter().map(|i| i.id_instancia).collect();
                aviso.validar_foco(&ids)?;
            }
        }
        mapas_compartidos(&est)
    };
    state.red.difundir(&aviso, &compartidos);
    Ok(())
}

// ========================================================
// IMÁGENES
// ========================================================

/// Directorio de datos de la app. El front lo usa para construir la URL de las imágenes
/// guardadas como ruta relativa (ver src/lib/imagenes.ts).
#[tauri::command]
pub fn obtener_dir_datos(state: State<'_, AppState>) -> String {
    state.almacen.dir().to_string_lossy().into_owned()
}

/// Copia la imagen elegida por el usuario al directorio de datos y devuelve su ruta relativa.
/// `uso` ("mapa" o "ficha") decide qué formatos se aceptan.
/// `uso` ("mapa" o "ficha") decide qué formatos se aceptan.
/// Es `async` + `spawn_blocking` para que copiar un archivo grande no congele la interfaz.
#[tauri::command]
pub async fn importar_imagen(origen: String, uso: imagenes::Uso, state: State<'_, AppState>) -> Result<String, String> {
    let dir = state.almacen.dir().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || imagenes::importar(&dir, Path::new(&origen), uso))
        .await
        .map_err(|e| format!("La importación falló: {e}"))?
}

// ========================================================
// FICHAS (entidades)
// ========================================================

/// Una ficha NUEVA recibe su id real en cuanto se pulsa «Añadir ficha» (o se arrastra una imagen),
/// aunque todavía no exista en el banco. Así sus archivos viven desde el primer momento en su carpeta
/// definitiva `archivos/entidad_<id>/` y no hay carpeta «temp» compartida.
///  * Guardar  -> `crear_entidad`, que la añade al banco con ese mismo id.
///  * Cancelar -> `cancelar_borrador_entidad`, que borra la carpeta y la reserva.
#[tauri::command]
pub fn iniciar_borrador_entidad(
    zona_id: u16,
    foto_url: Option<String>,
    state: State<'_, AppState>,
) -> Result<Entidad, String> {
    iniciar_borrador_entidad_en(zona_id, foto_url, &state)
}

/// Lo que hace `iniciar_borrador_entidad`, con el estado ya en la mano (así también lo ejecuta el host a petición de un jugador).
pub fn iniciar_borrador_entidad_en(zona_id: u16, foto_url: Option<String>, state: &AppState) -> Result<Entidad, String> {
    let est = state.lock();
    let mut reservas = state.reservas();

    if !est.zonas.iter().any(|z| z.id == zona_id) {
        return Err("Crea primero una zona para poder añadir fichas".into());
    }
    if let Some(f) = foto_url.as_deref().filter(|f| !f.trim().is_empty()) {
        archivos::ruta_segura(&state.almacen.dir(), f, &["imagenes"])?;
    }

    let id = recursos::siguiente_id_entidad(&est, &reservas);
    reservas.insert(id);

    // Si quedó una carpeta de una ficha que nunca se guardó (cierre brusco), este id está libre
    // y esos archivos no son de nadie: se limpia aquí, en el único momento en que es seguro.
    archivos::borrar_carpeta_entidad(&state.almacen.dir(), id);

    // Sin valores todavía: normalizar pone el de por defecto de cada parámetro
    let mut parametros = Vec::new();
    normalizar_parametros(&est.definiciones_parametros, &mut parametros);

    Ok(Entidad {
        id,
        nombre: "Nueva Ficha".into(),
        tipo: TipoEntidad::Enemigo,
        parametros,
        cuerpo: Cuerpo::Pequenio,
        zona_id,
        foto_url,
        miniatura_url: None,
        foto_encuadre: Default::default(),
        archivos_adjuntos: vec![],
        es_unico: false,
        propietarios: vec![],
    })
}

/// Cancela una ficha nueva: libera su id, borra su carpeta y descarta lo creado en la sesión
/// (la foto, etc.) siempre que la partida no lo use.
#[tauri::command]
pub fn cancelar_borrador_entidad(id: u16, rutas: Vec<String>, state: State<'_, AppState>) {
    cancelar_borrador_entidad_en(id, rutas, &state)
}

/// Lo que hace `cancelar_borrador_entidad`, con el estado ya en la mano.
pub fn cancelar_borrador_entidad_en(id: u16, rutas: Vec<String>, state: &AppState) {
    let est = state.lock();
    let mut reservas = state.reservas();

    if reservas.remove(&id) && !est.banco_entidades.iter().any(|e| e.id == id) {
        archivos::borrar_carpeta_entidad(&state.almacen.dir(), id);
    }
    recursos::descartar(&state.almacen.dir(), &est, &rutas);
}

/// Descarta archivos o imágenes creados durante una edición (al cancelar, o los que sobran al
/// guardar). Nunca borra nada que la partida esté usando.
#[tauri::command]
pub fn descartar_archivos_borrador(rutas: Vec<String>, state: State<'_, AppState>) {
    descartar_archivos_borrador_en(rutas, &state)
}

/// Lo que hace `descartar_archivos_borrador`, con el estado ya en la mano (así también lo ejecuta el host a petición de un jugador).
pub fn descartar_archivos_borrador_en(rutas: Vec<String>, state: &AppState) {
    let est = state.lock();
    recursos::descartar(&state.almacen.dir(), &est, &rutas);
}

/// Comprueba los parámetros de una ficha YA normalizada (uno por definición, en su orden).
fn validar_parametros(defs: &[DefinicionParametro], parametros: &[Parametro]) -> Result<(), String> {
    for (d, p) in defs.iter().zip(parametros) {
        if !p.valor.mismo_tipo(&d.valor_por_defecto) {
            return Err(format!("{}: el valor no es del tipo correcto", d.clave));
        }
        if d.tiene_maximo && !matches!(d.valor_por_defecto, ValorParametro::Numero(_)) {
            return Err(format!("{}: solo un número puede tener máximo", d.clave));
        }
        if matches!(p.valor, ValorParametro::Numero(n) if n < 0) {
            return Err(format!("{}: no puede ser negativo", d.clave));
        }
    }
    Ok(())
}

fn validar_entidad(est: &EstadoPartida, dir: &Path, e: &Entidad) -> Result<(), String> {
    if e.nombre.trim().is_empty() {
        return Err("La ficha necesita un nombre".into());
    }
    validar_parametros(&est.definiciones_parametros, &e.parametros)?;
    if let Cuerpo::Grande(dim) = &e.cuerpo {
        if dim.len() != 2 || dim.iter().any(|&d| d == 0) {
            return Err("El tamaño del cuerpo no es válido".into());
        }
    }
    if !est.zonas.iter().any(|z| z.id == e.zona_id) {
        return Err("La zona de la ficha ya no existe".into());
    }
    if let Some(f) = e.foto_url.as_deref().filter(|f| !f.trim().is_empty()) {
        archivos::ruta_segura(dir, f, &["imagenes"])?;
    }
    if !e.foto_encuadre.es_valido() {
        return Err("El encuadre de la foto no es válido".into());
    }
    for a in e.archivos_adjuntos.iter().flatten() {
        archivos::ruta_segura(dir, a, &["archivos"])?;
    }
    jugadores::validar_propietarios(est, &e.propietarios)?;
    Ok(())
}

/// Añade al banco una ficha iniciada con `iniciar_borrador_entidad` (su id ya es el definitivo).
#[tauri::command]
pub fn crear_entidad(entidad: Entidad, state: State<'_, AppState>) -> Result<Entidad, String> {
    crear_entidad_en(entidad, &state)
}

/// Lo que hace `crear_entidad`, con el estado ya en la mano (así también lo ejecuta el host a petición de un jugador).
pub fn crear_entidad_en(mut entidad: Entidad, state: &AppState) -> Result<Entidad, String> {
    let mut est = state.lock();
    let mut reservas = state.reservas();

    if !reservas.contains(&entidad.id) {
        return Err("Esta ficha no se inició con «Añadir ficha»".into());
    }
    if est.banco_entidades.iter().any(|e| e.id == entidad.id) {
        return Err("Esta ficha ya está guardada".into());
    }
    normalizar_parametros(&est.definiciones_parametros, &mut entidad.parametros);
    validar_entidad(&est, &state.almacen.dir(), &entidad)?;
    entidad.miniatura_url = None; // solo la pone `guardar_miniatura_ficha`

    reservas.remove(&entidad.id);
    registrar(&mut est, &state.almacen, DeltaPartida::EntidadCreada(entidad.clone()));
    Ok(entidad)
}

/// Guarda los cambios de una ficha. El front manda la ficha entera, pero el delta (`EntidadCambiada`) solo
/// lleva los campos que cambiaron; sin cambios no se registra nada. Los adjuntos que ya no están en la
/// lista se borran solos (recursos.rs).
#[tauri::command]
pub fn modificar_entidad(entidad: Entidad, state: State<'_, AppState>) -> Result<(), String> {
    modificar_entidad_en(entidad, &state)
}

/// Lo que hace `modificar_entidad`, con el estado ya en la mano (así también lo ejecuta el host a petición de un jugador).
pub fn modificar_entidad_en(mut entidad: Entidad, state: &AppState) -> Result<(), String> {
    let mut est = state.lock();

    let Some(guardada) = est.banco_entidades.iter().find(|e| e.id == entidad.id) else {
        return Err("La ficha ya no existe (¿se borró su zona?)".into());
    };
    // La miniatura propia es de Rust (la cambia `guardar_miniatura_ficha`): se conserva la guardada, y si
    // la foto es otra deja de valer (era un recorte de la anterior): se vuelve a la automática, centrada.
    if entidad.foto_url == guardada.foto_url {
        entidad.miniatura_url = guardada.miniatura_url.clone();
    } else {
        entidad.miniatura_url = None;
    }
    normalizar_parametros(&est.definiciones_parametros, &mut entidad.parametros);
    validar_entidad(&est, &state.almacen.dir(), &entidad)?;

    let Some(delta) = delta_de_cambios(&est, &entidad) else { return Ok(()) };
    registrar(&mut est, &state.almacen, delta);
    Ok(())
}

/// El delta que deja la ficha guardada como `entidad` (ya normalizada y validada): solo los campos que
/// cambian. `None` si no cambia nada o la ficha ya no existe.
fn delta_de_cambios(est: &EstadoPartida, entidad: &Entidad) -> Option<DeltaPartida> {
    let guardada = est.banco_entidades.iter().find(|e| e.id == entidad.id)?;
    let cambios = cambios_entidad::entre(guardada, entidad);
    (!cambios.is_empty()).then(|| DeltaPartida::EntidadCambiada { id: entidad.id, cambios })
}

/// Manda una ficha a la papelera con sus instancias (se usa la guardada, no la que manda el front).
/// Su foto y sus adjuntos siguen en disco hasta que se vacíe la papelera.
#[tauri::command]
pub fn borrar_entidad(entidad: Entidad, state: State<'_, AppState>) -> Result<(), String> {
    borrar_entidades(vec![entidad.id], state)
}

/// Manda varias fichas a la papelera de una vez (Supr con varias seleccionadas): un Ctrl+Z las devuelve todas.
#[tauri::command]
pub fn borrar_entidades(ids: Vec<u16>, state: State<'_, AppState>) -> Result<(), String> {
    let mut est = state.lock();
    let delta = papelera::borrar_entidades(&est, &ids)?;
    registrar(&mut est, &state.almacen, delta);
    Ok(())
}

/// Ctrl+Z: devuelve el último grupo de la papelera. `None` si no había nada que restaurar.
#[tauri::command]
pub fn restaurar_papelera(state: State<'_, AppState>) -> Result<Option<papelera::Restaurado>, String> {
    let mut est = state.lock();
    let Some((delta, restaurado)) = papelera::restaurar(&est)? else { return Ok(None) };
    registrar(&mut est, &state.almacen, delta);
    Ok(Some(restaurado))
}

/// Vacía la papelera (con el lock cogido): se liberan sus imágenes, miniaturas y adjuntos (recursos.rs)
/// y la carpeta de cada ficha que se va. Solo al cerrar la aplicación o al salir de la partida.
fn vaciar_papelera(est: &mut EstadoPartida, almacen: &Almacen) {
    if est.papelera.is_empty() {
        return;
    }
    let fichas = papelera::ids_entidades(est);
    registrar(est, almacen, DeltaPartida::PapeleraVaciada);
    for id in fichas.into_iter().filter(|id| !est.banco_entidades.iter().any(|e| e.id == *id)) {
        archivos::borrar_carpeta_entidad(&almacen.dir(), id);
    }
}

/// Al cerrar la ventana principal: se vacía la papelera y se guarda la partida.
pub fn cerrar_partida(state: &AppState) {
    // Los jugadores reciben «sala cerrada» y el túnel se para
    state.red.cerrar();
    let mut est = state.lock();
    vaciar_papelera(&mut est, &state.almacen);
    if let Err(e) = estado::compactar(&state.almacen, &est) {
        eprintln!("[partida] {e}");
    }
}

// ========================================================
// ARCHIVOS ADJUNTOS
// ========================================================

/// Solo se pueden adjuntar archivos a una ficha que existe o que se está creando.
fn comprobar_ficha_usable(state: &AppState, id: u16) -> Result<(), String> {
    let est = state.lock();
    let reservas = state.reservas();
    if est.banco_entidades.iter().any(|e| e.id == id) || reservas.contains(&id) {
        Ok(())
    } else {
        Err("La ficha no existe".into())
    }
}

#[tauri::command]
pub async fn importar_archivo(
    adjunto: ArchivoAdjunto,
    origen: String,
    state: State<'_, AppState>,
) -> Result<ArchivoAdjunto, String> {
    comprobar_ficha_usable(&state, adjunto.entidad_archivo.id)?;
    let dir = state.almacen.dir().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        archivos::importar(&dir, adjunto.entidad_archivo, Path::new(&origen))
    })
    .await
    .map_err(|e| format!("Fallo al importar archivo: {e}"))?
}

#[tauri::command]
pub fn crear_documento_texto(entidad: Entidad, state: State<'_, AppState>) -> Result<ArchivoAdjunto, String> {
    crear_documento_texto_en(entidad, &state)
}

/// Lo que hace `crear_documento_texto`, con el estado ya en la mano (así también lo ejecuta el host a petición de un jugador).
pub fn crear_documento_texto_en(entidad: Entidad, state: &AppState) -> Result<ArchivoAdjunto, String> {
    comprobar_ficha_usable(&state, entidad.id)?;
    archivos::crear_documento_texto(&state.almacen.dir(), entidad)
}

#[tauri::command]
pub fn guardar_archivo_texto(adjunto: ArchivoAdjunto,
    contenido: String, state: State<'_, AppState>) -> Result<ArchivoAdjunto, String> {
    guardar_archivo_texto_en(adjunto, contenido, &state)
}

/// Lo que hace `guardar_archivo_texto`, con el estado ya en la mano (así también lo ejecuta el host a petición de un jugador).
pub fn guardar_archivo_texto_en(adjunto: ArchivoAdjunto,
    contenido: String, state: &AppState) -> Result<ArchivoAdjunto, String> {
    archivos::guardar_texto(&state.almacen.dir(), adjunto, &contenido)
}

#[tauri::command]
pub fn leer_archivo_texto(ruta_relativa: String, state: State<'_, AppState>) -> Result<String, String> {
    leer_archivo_texto_en(ruta_relativa, &state)
}

/// Lo que hace `leer_archivo_texto`, con el estado ya en la mano (así también lo ejecuta el host a petición de un jugador).
pub fn leer_archivo_texto_en(ruta_relativa: String, state: &AppState) -> Result<String, String> {
    archivos::leer_texto(&state.almacen.dir(), &ruta_relativa)
}

/// Un archivo que la partida guardada ya usa se COPIA con el nombre nuevo (el original se libera
/// solo si se guarda la ficha, y no se toca si se cancela). Uno creado en esta edición se mueve.
#[tauri::command]
pub fn renombrar_archivo_adjunto(adjunto: ArchivoAdjunto,
    nuevo_nombre: String, state: State<'_, AppState>) -> Result<ArchivoAdjunto, String> {
    renombrar_archivo_adjunto_en(adjunto, nuevo_nombre, &state)
}

/// Lo que hace `renombrar_archivo_adjunto`, con el estado ya en la mano (así también lo ejecuta el host a petición de un jugador).
pub fn renombrar_archivo_adjunto_en(adjunto: ArchivoAdjunto,
    nuevo_nombre: String, state: &AppState) -> Result<ArchivoAdjunto, String> {
    let copiar = {
        let est = state.lock();
        recursos::esta_referenciada(&est, &adjunto.ruta)
    };
    archivos::renombrar(&state.almacen.dir(), adjunto, &nuevo_nombre, copiar)
}

/// Tipos que nunca se abren con el sistema desde la app (podrían ejecutar código).
const NO_ABRIR: [&str; 32] = [
    "exe", "bat", "cmd", "com", "scr", "msi", "msp", "ps1", "psm1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "hta",
    "cpl", "jar", "lnk", "url", "reg", "dll", "sh", "app", "appimage", "pif", "gadget", "msc", "scf", "apk", "bin", "run",
];

#[tauri::command]
pub fn abrir_archivo_en_sistema(
    app: tauri::AppHandle,
    ruta_relativa: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let path = archivos::ruta_segura(&state.almacen.dir(), &ruta_relativa, &["archivos"])?;
    if !path.is_file() {
        return Err("El archivo no existe".into());
    }
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if NO_ABRIR.contains(&extension.as_str()) {
        return Err(format!("Por seguridad no se abren archivos .{extension} desde la aplicación"));
    }

    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| format!("No se pudo abrir el archivo: {e}"))
}

// ========================================================
// MINIATURAS
// ========================================================

/// Ruta relativa de la miniatura WebP de una imagen (se genera la primera vez), o `None` si esa
/// imagen no se puede reducir y hay que usar la original. `tipo`: "ficha" o "mapa".
#[tauri::command]
pub async fn obtener_miniatura(
    ruta: String,
    tipo: String,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let tipo = Tipo::desde_texto(&tipo).ok_or("Tipo de miniatura desconocido")?;
    let dir = state.almacen.dir().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || miniaturas::asegurar(&dir, &ruta, tipo))
        .await
        .map_err(|e| format!("No se pudo generar la miniatura: {e}"))?
}

/// Guarda la miniatura propia de una ficha: el front recorta la foto con el encuadre del editor y manda el
/// WebP ya hecho; el archivo se guarda tal cual y el cambio entra como delta (`MiniaturaFichaCambiada`),
/// con lo que queda en el log y en el snapshot, y recursos.rs borra la miniatura anterior.
#[tauri::command]
pub fn guardar_miniatura_ficha(id: u16, datos: Vec<u8>, state: State<'_, AppState>) -> Result<(), String> {
    guardar_miniatura_ficha_en(id, datos, &state)
}

/// Lo que hace `guardar_miniatura_ficha`, con el estado ya en la mano (así también lo ejecuta el host a petición de un jugador).
pub fn guardar_miniatura_ficha_en(id: u16, datos: Vec<u8>, state: &AppState) -> Result<(), String> {
    let mut est = state.lock();
    if !est.banco_entidades.iter().any(|e| e.id == id) {
        return Err("La ficha no existe".into());
    }
    let ruta = miniaturas::guardar_de_ficha(&state.almacen.dir(), id, &datos)?;
    registrar(&mut est, &state.almacen, DeltaPartida::MiniaturaFichaCambiada { id, ruta: Some(ruta) });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn con_valores(valores: [ValorParametro; 4]) -> Vec<Parametro> {
        let defs = parametros_por_defecto();
        defs.iter()
            .zip(valores)
            .map(|(d, valor)| Parametro { id_definicion: d.id, valor, publico: false })
            .collect()
    }

    #[test]
    fn validar_parametros_acepta_los_de_serie_y_rechaza_tipos_y_negativos() {
        use ValorParametro::{Numero, Texto};
        let defs = parametros_por_defecto();
        assert!(validar_parametros(&defs, &con_valores([Numero(10), Numero(0), Numero(6), Numero(1)])).is_ok());
        assert_eq!(
            validar_parametros(&defs, &con_valores([Numero(10), Texto("x".into()), Numero(6), Numero(1)])),
            Err("Armadura: el valor no es del tipo correcto".into())
        );
        assert_eq!(
            validar_parametros(&defs, &con_valores([Numero(10), Numero(1), Numero(-1), Numero(1)])),
            Err("Velocidad: no puede ser negativo".into())
        );
    }

    fn ficha_valida(propietarios: Vec<IdJugador>) -> Entidad {
        let mut parametros = Vec::new();
        normalizar_parametros(&parametros_por_defecto(), &mut parametros);
        Entidad {
            id: 1,
            nombre: "Orco".into(),
            tipo: TipoEntidad::Enemigo,
            parametros,
            cuerpo: Cuerpo::Pequenio,
            zona_id: 1,
            foto_url: None,
            miniatura_url: None,
            foto_encuadre: Default::default(),
            archivos_adjuntos: vec![],
            es_unico: false,
            propietarios,
        }
    }

    fn estado_con_jugadores() -> EstadoPartida {
        let mut est = EstadoPartida::default();
        est.zonas = vec![Zona { id: 1, nombre: "Z".into(), orden: 0 }];
        est.jugadores = vec![Jugador { id: 1, nombre: "Ana".into() }];
        est
    }

    #[test]
    fn una_ficha_solo_puede_ser_de_jugadores_registrados() {
        let est = estado_con_jugadores();
        let dir = Path::new("datos");
        assert!(validar_entidad(&est, dir, &ficha_valida(vec![])).is_ok());
        assert!(validar_entidad(&est, dir, &ficha_valida(vec![1])).is_ok());
        for malos in [vec![7], vec![1, 1], vec![ID_HOST]] {
            assert!(validar_entidad(&est, dir, &ficha_valida(malos.clone())).is_err(), "{malos:?}");
        }
    }

    #[test]
    fn guardar_una_ficha_solo_registra_lo_que_cambia() {
        let mut est = estado_con_jugadores();
        est.banco_entidades = vec![ficha_valida(vec![])];

        assert!(delta_de_cambios(&est, &ficha_valida(vec![])).is_none(), "sin cambios no hay delta");

        let mut nueva = ficha_valida(vec![1]);
        nueva.nombre = "Orco jefe".into();
        let Some(DeltaPartida::EntidadCambiada { id, cambios }) = delta_de_cambios(&est, &nueva) else { panic!() };
        assert_eq!(id, 1);
        assert_eq!(
            cambios,
            vec![cambios_entidad::CambioEntidad::Nombre("Orco jefe".into()), cambios_entidad::CambioEntidad::Propietarios(vec![1])]
        );
        assert!(delta_de_cambios(&est, &Entidad { id: 9, ..nueva }).is_none(), "una ficha que ya no existe");
    }

    #[test]
    fn un_parametro_de_texto_no_puede_tener_maximo() {
        let defs = vec![DefinicionParametro {
            id: 9,
            clave: "Apodo".into(),
            valor_por_defecto: ValorParametro::Texto(String::new()),
            tiene_maximo: true,
            representar_mapa: None,
            visibilidad: Visibilidad::Variable,
            unidad: None,
            atenuar_si_negativo: false,
        }];
        let ps = vec![Parametro { id_definicion: 9, valor: ValorParametro::Texto("Bob".into()), publico: false }];
        assert!(validar_parametros(&defs, &ps).is_err());
    }
}

#[cfg(test)]
mod tests_definiciones {
    use super::*;

    fn nueva(clave: &str) -> DefinicionParametro {
        DefinicionParametro {
            id: 0,
            clave: clave.into(),
            valor_por_defecto: ValorParametro::Numero(1),
            tiene_maximo: false,
            representar_mapa: None,
            visibilidad: Visibilidad::Variable,
            unidad: None,
            atenuar_si_negativo: false,
        }
    }

    #[test]
    fn validar_definicion_rechaza_nombres_repetidos_vacios_y_combinaciones_absurdas() {
        let defs = parametros_por_defecto();
        assert!(validar_definicion(&defs, &nueva("Cordura")).is_ok());
        assert!(validar_definicion(&defs, &nueva("  ")).is_err());
        assert!(validar_definicion(&defs, &nueva(" vida ")).is_err(), "repetido sin mirar mayúsculas ni espacios");

        let texto_con_maximo = DefinicionParametro {
            valor_por_defecto: ValorParametro::Texto("x".into()),
            tiene_maximo: true,
            ..nueva("Apodo")
        };
        assert!(validar_definicion(&defs, &texto_con_maximo).is_err());

        let color_malo = DefinicionParametro { representar_mapa: Some(Forma::Circulo(vec![1, 2, 3])), ..nueva("Aura") };
        assert!(validar_definicion(&defs, &color_malo).is_err());
        let texto_en_mapa = DefinicionParametro {
            valor_por_defecto: ValorParametro::Texto("x".into()),
            representar_mapa: Some(Forma::Circulo(vec![1, 2, 3, 4])),
            ..nueva("Lema")
        };
        assert!(validar_definicion(&defs, &texto_en_mapa).is_err());

        let resumen = |tiene_maximo| DefinicionParametro {
            representar_mapa: Some(Forma::Resumen(vec![1, 2, 3, 4])),
            tiene_maximo,
            ..nueva("Maná")
        };
        assert!(validar_definicion(&defs, &resumen(true)).is_ok());
        assert!(validar_definicion(&defs, &resumen(false)).is_err(), "Resumen sin máximo");

        let atenua = |tiene_maximo| DefinicionParametro { atenuar_si_negativo: true, tiene_maximo, ..nueva("Aguante") };
        assert!(validar_definicion(&defs, &atenua(true)).is_ok());
        assert!(validar_definicion(&defs, &atenua(false)).is_err(), "atenuar sin máximo");
    }
}

#[cfg(test)]
mod tests_partidas {
    use super::*;
    use crate::tablero_general::almacen::pruebas::dir_temporal;

    fn zona(id: u16) -> Zona {
        Zona { id, nombre: format!("Z{id}"), orden: 0 }
    }

    /// Una raíz con las partidas «A» y «B», abierta en «A».
    fn estado_en_a(nombre: &str) -> AppState {
        let raiz = dir_temporal(nombre);
        std::fs::create_dir_all(&raiz).unwrap();
        partidas::crear(&raiz, "A").unwrap();
        partidas::crear(&raiz, "B").unwrap();
        let almacen = Almacen::abrir(partidas::ruta_partida(&raiz, "A")).unwrap();
        AppState::nuevo(EstadoPartida::default(), almacen, raiz)
    }

    fn zonas(state: &AppState) -> Vec<u16> {
        state.lock().zonas.iter().map(|z| z.id).collect()
    }

    #[test]
    fn alternar_guarda_la_que_se_deja_y_no_mezcla_partidas() {
        let state = estado_en_a("alternar");
        procesar_delta(&state, DeltaPartida::ZonaCreada(zona(1)));
        procesar_delta(&state, DeltaPartida::ParametroBorrado(ID_VIDA)); // A tiene 3 parámetros

        cambiar_de_partida(&state, "B").unwrap();
        assert_eq!(nombre_partida_actual(&state), "B");
        assert!(zonas(&state).is_empty(), "B está vacía");
        assert_eq!(state.lock().definiciones_parametros.len(), 4, "B nace con los parámetros de serie");
        let dir_a = partidas::ruta_partida(&state.raiz, "A");
        assert!(dir_a.join("partida.json").is_file(), "A se guardó antes de cambiar");
        assert_eq!(std::fs::read_to_string(dir_a.join("deltas.jsonl")).unwrap(), "", "y su log quedó vacío");

        // Un cambio en B va al log de B, no al de A
        procesar_delta(&state, DeltaPartida::ZonaCreada(zona(2)));
        assert!(std::fs::read_to_string(partidas::ruta_partida(&state.raiz, "B").join("deltas.jsonl")).unwrap().contains("ZonaCreada"));
        assert_eq!(std::fs::read_to_string(dir_a.join("deltas.jsonl")).unwrap(), "");

        // Volver (sin distinguir mayúsculas) deja A como estaba, sin lo de B
        cambiar_de_partida(&state, "a").unwrap();
        assert_eq!(zonas(&state), [1]);
        assert_eq!(state.lock().definiciones_parametros.len(), 3);
        cambiar_de_partida(&state, "B").unwrap();
        assert_eq!(zonas(&state), [2], "lo de B también se guardó");
        assert_eq!(partidas::leer_ultima(&state.raiz).as_deref(), Some("B"));
    }

    #[test]
    fn no_se_cambia_con_una_ficha_nueva_abierta_ni_a_una_partida_que_no_existe() {
        let state = estado_en_a("cambio_bloqueado");
        procesar_delta(&state, DeltaPartida::ZonaCreada(zona(1)));

        state.reservas().insert(7);
        assert!(cambiar_de_partida(&state, "B").unwrap_err().contains("Cierra la ficha"));
        state.reservas().clear();

        // Con la sala abierta tampoco: sus jugadores están en esta partida
        state.red.abrir_sin_servidor();
        assert!(cambiar_de_partida(&state, "B").unwrap_err().contains("Cierra la sala"));
        state.red.cerrar();

        for mala in ["C", "../A", "..", ""] {
            assert!(cambiar_de_partida(&state, mala).is_err(), "debía rechazar {mala:?}");
        }
        // Nada cambió: sigue en A con su zona
        assert_eq!(nombre_partida_actual(&state), "A");
        assert_eq!(zonas(&state), [1]);
        // Abrir la que ya está abierta no hace nada
        cambiar_de_partida(&state, "A").unwrap();
        assert_eq!(zonas(&state), [1]);
    }

    #[test]
    fn una_partida_nueva_se_crea_y_se_abre_con_los_parametros_de_serie() {
        let state = estado_en_a("crear_y_abrir");
        procesar_delta(&state, DeltaPartida::ParametroBorrado(ID_VIDA));
        let nombre = partidas::crear(&state.raiz, "  Tercera ").unwrap();
        cambiar_de_partida(&state, &nombre).unwrap();
        assert_eq!(nombre_partida_actual(&state), "Tercera");
        // Sus parámetros ya están en su partida.json, no solo en memoria
        let guardado = std::fs::read_to_string(partidas::ruta_partida(&state.raiz, "Tercera").join("partida.json")).unwrap();
        assert!(guardado.contains("Rango arma"));
        assert_eq!(state.lock().definiciones_parametros.len(), 4);
        assert!(partidas::crear(&state.raiz, "tercera").is_err(), "repetida");
    }
}

#[cfg(test)]
mod tests_fondos_antiguos {
    use super::*;
    use crate::tablero_general::almacen::pruebas::dir_temporal;

    #[test]
    fn reconoce_las_urls_antiguas_y_las_decodifica() {
        assert_eq!(
            ruta_de_url_antigua("http://asset.localhost/C%3A%5CMapas%5Cel%20desierto%5C1-1.png"),
            Some(PathBuf::from(r"C:\Mapas\el desierto\1-1.png"))
        );
        assert_eq!(ruta_de_url_antigua("asset://localhost/%2Fhome%2Fana%2Fmapa.png"), Some(PathBuf::from("/home/ana/mapa.png")));
        assert_eq!(ruta_de_url_antigua("imagenes/abc.png"), None);
        assert_eq!(ruta_de_url_antigua("https://example.com/mapa.png"), None);
    }

    fn mapa(id: u16, fondo: Option<String>) -> Mapa {
        Mapa {
            id, id_zona: 1, nombre: id.to_string(), url_imagen_fondo: fondo, ancho: 1000, alto: 1000, rejilla: 50,
            color_opacidad: vec![255, 255, 255, 76], grosor: 1, orden: 0, compartido: false,
        }
    }

    #[test]
    fn el_fondo_antiguo_pasa_a_imagenes_con_su_tamano_y_lo_que_no_se_puede_se_deja() {
        let dir = dir_temporal("fondos_antiguos");
        let origen = dir_temporal("fondos_antiguos_o");
        std::fs::create_dir_all(&origen).unwrap();
        image::RgbaImage::new(40, 30).save(origen.join("mi mapa.png")).unwrap();
        let url = format!("http://asset.localhost/{}", origen.join("mi mapa.png").to_string_lossy().replace('\\', "%5C").replace(' ', "%20").replace(':', "%3A"));
        let perdida = "http://asset.localhost/C%3A%5Cno%5Cexiste.png".to_string();

        let almacen = Almacen::abrir(&dir).unwrap();
        let mut est = EstadoPartida::default();
        let zona = Zona { id: 1, nombre: "Z".into(), orden: 0 };
        registrar(&mut est, &almacen, DeltaPartida::ZonaCreada(zona.clone()));
        registrar(&mut est, &almacen, DeltaPartida::MapaCreado(zona.clone(), mapa(1, Some(url))));
        registrar(&mut est, &almacen, DeltaPartida::MapaCreado(zona.clone(), mapa(2, Some(perdida.clone()))));
        registrar(&mut est, &almacen, DeltaPartida::MapaCreado(zona.clone(), mapa(3, Some("imagenes/ya.png".into()))));
        registrar(&mut est, &almacen, DeltaPartida::MapaCreado(zona, mapa(4, None)));

        migrar_fondos_antiguos(&mut est, &almacen);
        corregir_tamanos_mapas(&mut est, &almacen);

        let fondo = |id: u16| est.mapas.iter().find(|m| m.id == id).unwrap().url_imagen_fondo.clone();
        let nuevo = fondo(1).unwrap();
        assert!(nuevo.starts_with("imagenes/") && nuevo.ends_with(".png"), "{nuevo}");
        assert!(dir.join(&nuevo).is_file(), "se copió a la carpeta de datos");
        let m1 = est.mapas.iter().find(|m| m.id == 1).unwrap();
        assert_eq!((m1.ancho, m1.alto), (40, 30), "y ya se pudo medir");
        assert_eq!(fondo(2), Some(perdida), "el archivo ya no existe: se deja");
        assert_eq!(fondo(3).as_deref(), Some("imagenes/ya.png"), "una ruta normal no se toca");
        assert_eq!(fondo(4), None);

        // Queda en el log como cualquier cambio: al reabrir, sale igual
        let (releido, _) = estado::cargar(&almacen).unwrap();
        assert_eq!(releido.mapas.iter().find(|m| m.id == 1).unwrap().url_imagen_fondo.as_deref(), Some(nuevo.as_str()));
    }
}
