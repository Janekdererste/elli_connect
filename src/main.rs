use axum::{
    routing::{get, get_service},
    Router,
    response::Html,
    extract::ws::{WebSocket, WebSocketUpgrade},
};
use std::net::SocketAddr;
use tower_http::services::ServeDir;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

// Handler for the root path
async fn root() -> Html<&'static str> {
    Html("<h1>Welcome to Elli Spotify API</h1>")
}

// WebSocket handler
async fn ws_handler(ws: WebSocketUpgrade) -> impl axum::response::IntoResponse {
    ws.on_upgrade(handle_socket)
}

// WebSocket connection handler
async fn handle_socket(socket: WebSocket) {
    // TODO: Implement WebSocket connection handling
    // This is where you'll handle the socket connection to other services
}

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Build our application with a route
    let app = Router::new()
        .route("/", get(root))
        .route("/ws", get(ws_handler))
        .nest_service(
            "/static",
            get_service(ServeDir::new("static")).handle_error(|error: std::io::Error| async move {
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Unhandled internal error: {}", error),
                )
            }),
        );

    // Run it with hyper on localhost:3000
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::info!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
