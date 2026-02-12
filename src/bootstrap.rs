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
};
use tower::ServiceBuilder;

use crate::{
    cfg,
    integration::{Command, cache},
    middleware::{extract_identifier, extract_ip, lease_tokens},
    state, store,
};

use dotenv::dotenv;

pub fn init_router(s: state::AppState) -> Router {
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
        .merge(protected)
        .with_state(s)
}

pub struct App {
    shutdown_tx: Option<Sender<Command>>,
    state: state::AppState,
}

impl App {
    pub async fn new() -> crate::Result<Self> {
        if let Err(e) = dotenv() {
            eprintln!("Could not initialaize dotenv, {e:?}");
        }

        init_logger();

        let cfg = env::var("CFG_PATH").map(|path| cfg::get(&path).unwrap_or_default())?;
        let redis = cache::Config::env().unwrap_or_default().connect().await;

        let shutdown_tx = if cfg.cleanup.enabled {
            let (tx, _) = broadcast::channel::<Command>(10);
            Some(tx)
        } else {
            None
        };

        let store = store::Store::new(cfg, redis, shutdown_tx.clone());
        let state = state::AppState::new(store);

        Ok(Self { shutdown_tx, state })
    }

    pub async fn start(&self) {
        let port = env::var("SERVER_PORT").unwrap_or_else(|_| "8000".to_string());
        let listener = match tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await {
            Ok(l) => l,
            Err(e) => panic!("Failed to start application: {e:?}"),
        };

        info!("Starting on port: {port}");

        let r = init_router(self.state.clone());

        let shutdown_tx = self.shutdown_tx.clone();
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

async fn shutdown_signal(tx: Option<Sender<Command>>) {
    if let Some(tx) = tx {
        let mut rx = tx.subscribe();
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

        let _ = tx.send(Command::Shutdown);

        let mut interval = tokio::time::interval(Duration::from_secs(5));

        loop {
            tokio::select! {
                _ = interval.tick() => {},
                Ok(Command::CleanupComplete) = rx.recv() => {
                    debug!("Cleanup complete, stopping application");
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
pub mod test {

    use std::sync::Arc;

    use testcontainers_modules::testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner};
    use tokio::sync::broadcast;

    use crate::{
        bootstrap::init_test_logger,
        cfg::Config,
        integration::{Command, cache},
        state::AppState,
        store,
    };

    /// Wrapper App to keep redis container alive
    /// for the whole duration of the test.
    pub struct TestApp {
        inner: AppState,
        store: Arc<store::Store>,
        cfg: Config,
        _redis_container: Arc<ContainerAsync<testcontainers_modules::redis::Redis>>,
    }

    impl TestApp {
        pub async fn new() -> Self {
            Self::with_cfg(Config::default()).await
        }

        pub async fn with_cfg(cfg: Config) -> Self {
            init_test_logger();

            let rc = testcontainers_modules::redis::Redis::default()
                .with_tag("7")
                .start()
                .await
                .map(Arc::new)
                .unwrap();
            let host = rc.get_host().await.unwrap().to_string();
            let port = rc.get_host_port_ipv4(6379).await.unwrap();

            let shutdown_tx = if cfg.cleanup.enabled {
                let (tx, _) = broadcast::channel::<Command>(10);
                Some(tx)
            } else {
                None
            };

            let redis = cache::Config::test(host, port).connect().await;
            let store = store::Store::new(cfg.clone(), redis, shutdown_tx);

            Self {
                inner: AppState::new(store.clone()),
                store,
                cfg,
                _redis_container: rc,
            }
        }

        pub const fn app_state(&self) -> &AppState {
            &self.inner
        }

        pub fn store(&self) -> Arc<store::Store> {
            self.store.clone()
        }

        pub const fn config(&self) -> &Config {
            &self.cfg
        }
    }
}
