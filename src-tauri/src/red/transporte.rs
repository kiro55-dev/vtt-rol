// src-tauri/src/red/transporte.rs
//
// Por dónde se habla con UN jugador, sin que `servidor::atender` sepa si es un WebSocket (el túnel) o, más adelante,
// un data channel de WebRTC: dos colas de `Trama` (texto = un mensaje JSON del protocolo, binario = un trozo de
// imagen). Cada vía tiene su adaptador (`desde_websocket`; en la fase 3, el de WebRTC) que pasa del socket a estas
// colas y al revés, y que se ocupa de lo suyo (los pings del WebSocket, por ejemplo).
//
//  * La cola de SALIDA está acotada: es la contrapresión. Quien envía (el bucle de `atender`, la tarea que manda
//    una imagen) espera si el jugador no lee al ritmo, y si no puede meter nada en `TIEMPO_ENVIO` se le corta.
//    Así una imagen grande va al ritmo de la conexión del jugador sin acumular memoria en el host.
//  * Soltar el `Transporte` cierra la conexión: el adaptador ve la cola de salida cerrada y cierra el socket.
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::time::timeout;

/// Mensajes que caben en cada cola. Con la de salida llena, quien envía espera.
pub const CAPACIDAD: usize = 32;
/// Tiempo máximo para poder meter un mensaje en la salida (el jugador no lee): pasado esto se le corta.
pub const TIEMPO_ENVIO: Duration = if cfg!(test) { Duration::from_secs(2) } else { Duration::from_secs(30) };
/// Cloudflare corta un WebSocket sin tráfico a los 100 s.
const CADA_PING: Duration = Duration::from_secs(30);

/// Lo que viaja en un sentido o en otro.
#[derive(Debug, Clone, PartialEq)]
pub enum Trama {
    /// Un mensaje del protocolo, en JSON (`protocolo.rs`).
    Texto(String),
    /// Un trozo de imagen (ver `red::imagenes`).
    Binario(Vec<u8>),
}

/// La conexión con un jugador, vista como dos colas.
pub struct Transporte {
    /// Hacia el jugador (acotada: ver arriba).
    pub tx: mpsc::Sender<Trama>,
    /// Lo que llega del jugador. `None` al cerrarse la conexión.
    pub rx: mpsc::Receiver<Trama>,
}

/// Mete una trama en la salida esperando como mucho `TIEMPO_ENVIO`. `Err` si la conexión se cerró o el jugador no lee.
pub async fn enviar_trama(tx: &mpsc::Sender<Trama>, t: Trama) -> Result<(), ()> {
    match timeout(TIEMPO_ENVIO, tx.send(t)).await {
        Ok(Ok(())) => Ok(()),
        _ => Err(()),
    }
}

/// Envía un mensaje del protocolo (en JSON). Igual que `enviar_trama`.
pub async fn enviar_mensaje(tx: &mpsc::Sender<Trama>, m: &crate::red::protocolo::MensajeHost) -> Result<(), ()> {
    let json = serde_json::to_string(m).map_err(|_| ())?;
    enviar_trama(tx, Trama::Texto(json)).await
}

/// Adapta un WebSocket: una tarea lo lee y otra le escribe (y le manda un ping cada 30 s).
pub fn desde_websocket(socket: WebSocket) -> Transporte {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (tx, mut salida) = mpsc::channel::<Trama>(CAPACIDAD);
    let (entrada, rx) = mpsc::channel::<Trama>(CAPACIDAD);

    tokio::spawn(async move {
        let mut ping = tokio::time::interval(CADA_PING);
        ping.tick().await;
        loop {
            tokio::select! {
                t = salida.recv() => {
                    let mensaje = match t {
                        Some(Trama::Texto(s)) => Message::Text(s.into()),
                        Some(Trama::Binario(b)) => Message::Binary(b.into()),
                        None => break, // se soltó el `Transporte`: se cierra
                    };
                    if ws_tx.send(mensaje).await.is_err() {
                        break;
                    }
                }
                _ = ping.tick() => {
                    if ws_tx.send(Message::Ping(Bytes::new())).await.is_err() {
                        break;
                    }
                }
            }
        }
        let _ = ws_tx.close().await;
    });

    tokio::spawn(async move {
        while let Some(Ok(m)) = ws_rx.next().await {
            let trama = match m {
                Message::Text(t) => Trama::Texto(t.to_string()),
                Message::Binary(b) => Trama::Binario(b.to_vec()),
                Message::Close(_) => break,
                Message::Ping(_) | Message::Pong(_) => continue,
            };
            if entrada.send(trama).await.is_err() {
                break; // `atender` ya terminó
            }
        }
    });

    Transporte { tx, rx }
}

/// Dos extremos unidos en memoria (para los tests: uno hace de host y el otro de jugador).
#[cfg(test)]
pub fn en_memoria() -> (Transporte, Transporte) {
    let (a_tx, a_rx) = mpsc::channel(CAPACIDAD);
    let (b_tx, b_rx) = mpsc::channel(CAPACIDAD);
    (Transporte { tx: a_tx, rx: b_rx }, Transporte { tx: b_tx, rx: a_rx })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn en_memoria_lo_que_manda_uno_lo_recibe_el_otro() {
        let (mut host, mut jugador) = en_memoria();
        enviar_trama(&host.tx, Trama::Texto("hola".into())).await.unwrap();
        assert_eq!(jugador.rx.recv().await, Some(Trama::Texto("hola".into())));
        enviar_trama(&jugador.tx, Trama::Binario(vec![1, 2])).await.unwrap();
        assert_eq!(host.rx.recv().await, Some(Trama::Binario(vec![1, 2])));
        drop(jugador);
        assert_eq!(host.rx.recv().await, None, "al soltar un extremo, el otro ve la conexión cerrada");
    }

    #[tokio::test]
    async fn si_el_otro_no_lee_el_envio_se_rinde() {
        let (host, _jugador_que_no_lee) = en_memoria();
        for _ in 0..CAPACIDAD {
            enviar_trama(&host.tx, Trama::Texto("x".into())).await.unwrap();
        }
        let t0 = std::time::Instant::now();
        assert!(enviar_trama(&host.tx, Trama::Texto("x".into())).await.is_err(), "cola llena y nadie lee");
        assert!(t0.elapsed() >= TIEMPO_ENVIO);
    }
}
