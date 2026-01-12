use axum::{
    Router,
    http::StatusCode,
    middleware::{from_fn, from_fn_with_state},
    routing::get,
};
use tower::ServiceBuilder;

use crate::middleware::{extract_user_id, lease_tokens};

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

    let protected = Router::new()
        .route("/consume", get(store::handler::consume))
        .route("/check", get(store::handler::check))
        .route_layer(
            ServiceBuilder::new()
                .layer(from_fn(extract_user_id))
                .layer(from_fn_with_state(state.clone(), lease_tokens)),
        );

    let app = Router::new()
        .route("/health", get(|| async { (StatusCode::OK, "UP") }))
        .route("/config", get(cfg::handler::get))
        .merge(protected)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
