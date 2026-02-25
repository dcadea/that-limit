use std::{
    env::{self, VarError},
    str::FromStr,
    time::Duration,
};

use log::debug;

use dotenv::dotenv;
use simplelog::{ColorChoice, LevelFilter, TermLogger, TerminalMode};
use that_limit_cache::CacheConfig;
use that_limit_core::{Command, Store};
use tokio::{
    signal,
    sync::broadcast::{self, Sender},
};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    VarError(#[from] VarError),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(not(any(feature = "envoy", feature = "http")))]
compile_error!("Either feature `envoy` or `http` must be enabled.");

#[tokio::main]
async fn main() -> Result<()> {
    if let Err(e) = dotenv() {
        eprintln!("Could not initialaize dotenv, {e:?}");
    }

    init_logger();

    let config =
        env::var("CFG_PATH").map(|path| that_limit_core::get(&path).unwrap_or_default())?;
    let redis = CacheConfig::env().unwrap_or_default().connect().await;

    let shutdown_tx = if config.cleanup.enabled {
        let (tx, _) = broadcast::channel::<Command>(10);
        Some(tx)
    } else {
        None
    };

    let store = Store::new(config, redis, shutdown_tx.clone());

    #[cfg(all(feature = "envoy", not(feature = "http")))]
    {
        use that_limit_envoy_adapter::start_envoy;
        start_envoy(store, shutdown_signal(shutdown_tx.clone())).await;
    }

    #[cfg(all(feature = "http", not(feature = "envoy")))]
    {
        use that_limit_http_adapter::start_http;
        start_http(store, shutdown_signal(shutdown_tx.clone())).await;
    }
    Ok(())
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

async fn shutdown_signal(tx: Option<Sender<Command>>) {
    if let Some(tx) = tx {
        let mut rx = tx.subscribe();

        #[cfg(unix)]
        {
            use log::error;

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
        }

        #[cfg(not(unix))]
        tokio::select! {
            _ = signal::ctrl_c() => {
                debug!("Ctrl-C received, shutting down...");
            }
        }

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
