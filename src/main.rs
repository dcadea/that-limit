use axum::{Router, http::StatusCode, routing::get};

use crate::middleware::extract_user_id;

mod cfg;
mod middleware;
mod state;
mod store;

#[tokio::main]
async fn main() {
    let cfg = cfg::get("static/config.json").unwrap();

    let state = state::AppState::new(cfg);

    let router_with_middleware = Router::new()
        .route("/consume", get(store::handler::consume))
        .route("/check", get(store::handler::check))
        .layer(axum::middleware::from_fn(extract_user_id));

    let app = Router::new()
        .route("/health", get(|| async { (StatusCode::OK, "UP") }))
        .route("/config", get(cfg::handler::get))
        .merge(router_with_middleware)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
