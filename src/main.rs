use axum::{Router, http::StatusCode, routing::get};

mod cfg;
mod state;

#[tokio::main]
async fn main() {
    let state = state::AppState::new();

    let app = Router::new()
        .route("/health", get(|| async { (StatusCode::OK, "UP") }))
        .route("/config", get(cfg::handler::get))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
