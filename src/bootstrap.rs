use std::{env, str::FromStr, sync::Arc, time::Duration};

use log::{LevelFilter, debug, error};
use simplelog::{ColorChoice, TermLogger, TerminalMode};
use tokio::{
    signal,
    sync::broadcast::{self, Sender},
};

use crate::core::{
    cfg,
    integration::{Command, cache},
    store::Store,
};

use dotenv::dotenv;

pub struct App {
    pub shutdown_tx: Option<Sender<Command>>,
    store: Arc<Store>,
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

        let store = Store::new(cfg, redis, shutdown_tx.clone());

        Ok(Self { shutdown_tx, store })
    }

    pub fn store(&self) -> Arc<Store> {
        self.store.clone()
    }

    pub async fn run(&self) {
        #[cfg(all(feature = "grpc", feature = "http"))]
        tokio::select! {
            _ = self.run_grpc() => {}
            _ = self.run_http() => {}
        }

        #[cfg(all(feature = "grpc", not(feature = "http")))]
        self.run_grpc().await;

        #[cfg(all(feature = "http", not(feature = "grpc")))]
        self.run_http().await;
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
pub fn init_test_logger() {
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

pub async fn shutdown_signal(tx: Option<Sender<Command>>) {
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
