use std::{env, net::SocketAddr, str::FromStr, time::Duration};

use axum::{
    Router,
    http::StatusCode,
    middleware::{from_fn, from_fn_with_state},
    routing::{get, post},
};
use log::{LevelFilter, debug, error, info};
use simplelog::{ColorChoice, TermLogger, TerminalMode};
use tokio::{
    signal,
    sync::broadcast::{self, Sender},
    time::sleep,
};
use tower::ServiceBuilder;

use crate::{
    middleware::{extract_identifier, extract_ip, lease_tokens},
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

    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    let s = state::AppState::new(shutdown_tx.clone()).await?;

    let r = init_router(s.clone());

    start(r, shutdown_tx).await;

    Ok(())
}

fn init_router(s: AppState) -> Router {
    let protected = Router::new()
        .route(
            "/consume",
            post(store::handler::consume).layer(from_fn_with_state(s.clone(), lease_tokens)),
        )
        .route("/check", get(store::handler::check))
        .route_layer(
            ServiceBuilder::new()
                .layer(from_fn(extract_ip))
                .layer(from_fn(extract_identifier)),
        );

    Router::new()
        .route("/health", get(|| async { (StatusCode::OK, "UP") }))
        .route("/config", get(cfg::handler::get))
        .merge(protected)
        .with_state(s)
}

async fn start(r: Router, shutdown_tx: Sender<()>) {
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
    .with_graceful_shutdown(shutdown_signal(shutdown_tx))
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

async fn shutdown_signal(shutdown_tx: Sender<()>) {
    #[cfg(unix)]
    let unix_signal = async {
        use tokio::signal;

        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut sigterm) => {
                sigterm.recv().await;
            }
            Err(e) => {
                error!("Failed to listen for SIGTERM: {e}");
            }
        }
    };

    tokio::select! {
        _ = signal::ctrl_c() => {
            debug!("Ctrl-C received, shutting down...");
        },
        () = unix_signal => {
            debug!("SIGTERM received, shutting down...");
        }
    }

    debug!("Shutdown signal received");
    let _ = shutdown_tx.send(());

    // give background tasks time to cleanup
    sleep(Duration::from_millis(100)).await;
}

/// If more than one tests are executed at once, each of them
/// might want to initialize logger.
#[cfg(test)]
fn init_test_logger() {
    // Ignore error, most likely already initialized by another test
    if let Err(_) = TermLogger::init(
        LevelFilter::Debug,
        simplelog::Config::default(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    ) {
        // NOOP
    }
}
