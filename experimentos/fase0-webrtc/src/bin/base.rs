//! Solo axum + tokio: para medir cuánto añade cada librería WebRTC al ejecutable.
#[tokio::main]
async fn main() {
    let app = axum::Router::new().route("/", axum::routing::get(|| async { "base" }));
    let escucha = tokio::net::TcpListener::bind("127.0.0.1:8091").await.unwrap();
    axum::serve(escucha, app).await.unwrap();
}
