use axum::{Router, routing::get};

#[tokio::main]
async fn main() {
    console_subscriber::init();

    let app = Router::new().route("/", get(|| async { "Hello, World!" }));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:4004").await.unwrap();

    tracing::info!("🐗 Listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}
