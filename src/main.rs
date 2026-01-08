use axum::{Router, http::StatusCode, routing::get};

use crate::middleware::extract_user_id;

mod bucket;
mod cfg;
mod error;
mod integration;
mod middleware;
mod state;
mod store;

pub type Result<T> = std::result::Result<T, crate::error::Error>;

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = cfg::get("static/config.json")?;

    let state = state::AppState::new(cfg).await;

    let router_with_middleware = Router::new()
        .route("/consume", get(store::handler::consume))
        .route("/check", get(store::handler::check))
        .layer(axum::middleware::from_fn(extract_user_id));

    let app = Router::new()
        .route("/health", get(|| async { (StatusCode::OK, "UP") }))
        .route("/config", get(cfg::handler::get))
        .merge(router_with_middleware)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
