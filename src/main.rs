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

    let app = Router::new()
        .route("/health", get(|| async { (StatusCode::OK, "UP") }))
        .route("/config", get(cfg::handler::get))
        .route(
            "/consume",
            get(store::handler::consume).layer(axum::middleware::from_fn(extract_user_id)),
        )
        .route(
            "/check",
            get(store::handler::check).layer(axum::middleware::from_fn(extract_user_id)),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
