use axum::{
    routing::{get, get_service},
    Router,
    extract::ws::{WebSocket, WebSocketUpgrade},
};
use std::net::SocketAddr;
use tower_http::services::ServeDir;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Build our application with routes
    let app = Router::new()
        // Serve index.html at the root
        .nest_service("/", get_service(ServeDir::new("static")))
        .route("/ws", get(ws_handler));

    // Run it with hyper on localhost:3000
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::info!("listening on {}", addr);
    
    axum::serve(
        tokio::net::TcpListener::bind(addr).await.unwrap(),
        app
    ).await.unwrap();
}

// WebSocket handler
async fn ws_handler(ws: WebSocketUpgrade) -> impl axum::response::IntoResponse {
    ws.on_upgrade(handle_socket)
}

// WebSocket connection handler
async fn handle_socket(_socket: WebSocket) {
    // TODO: Implement WebSocket connection handling
    // This is where you'll handle the socket connection to other services
}
