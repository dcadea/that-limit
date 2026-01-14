use std::{env, net::SocketAddr, str::FromStr};

use axum::{
    Router,
    http::StatusCode,
    middleware::{from_fn, from_fn_with_state},
    routing::get,
};
use axum_client_ip::ClientIpSource;
use log::{LevelFilter, info};
use simplelog::{ColorChoice, TermLogger, TerminalMode};
use tower::ServiceBuilder;

use crate::{
    middleware::{extract_identifier, lease_tokens},
    state::AppState,
};

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
    init_logger();

    let s = state::AppState::new().await?;
    let r = init_router(s);

    start(r).await;

    Ok(())
}

fn init_router(s: AppState) -> Router {
    let protected = Router::new()
        .route("/consume", get(store::handler::consume))
        .route("/check", get(store::handler::check))
        .route_layer(
            ServiceBuilder::new()
                .layer(from_fn(extract_identifier))
                .layer(from_fn_with_state(s.clone(), lease_tokens)),
        );

    Router::new()
        .route("/health", get(|| async { (StatusCode::OK, "UP") }))
        .route("/config", get(cfg::handler::get))
        .merge(protected)
        .route_layer(
            ServiceBuilder::new()
                .layer(ClientIpSource::XRealIp.into_extension())
                .layer(ClientIpSource::RightmostForwarded.into_extension())
                .layer(ClientIpSource::RightmostXForwardedFor.into_extension()),
        )
        .with_state(s)
}

async fn start(r: Router) {
    let port = env::var("SERVER_PORT").unwrap_or_else(|_| "8000".to_string());
    let listener = match tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await {
        Ok(l) => l,
        Err(e) => panic!("Failed to start application: {e:?}"),
    };

    info!("Starting on port: {port}");

    if let Err(e) = axum::serve(
        listener,
        r.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    {
        panic!("Failed to start application: {e:?}")
    }
}

fn init_logger() {
    let rust_log = env::var("RUST_LOG").unwrap_or_else(|_| "info".into());
    let level = LevelFilter::from_str(&rust_log).unwrap_or(LevelFilter::Info);

    TermLogger::init(
        level,
        simplelog::Config::default(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )
    .expect("Failed to initialize logger");
}
