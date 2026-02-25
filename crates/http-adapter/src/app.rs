use std::{env, net::SocketAddr, sync::Arc};

use axum::{
    Router,
    http::StatusCode,
    middleware::from_fn,
    routing::{get, post},
};
use log::info;
use that_limit_core::Store;
use tower::ServiceBuilder;

use crate::{
    middleware::{extract_identifier, extract_ip},
    state::AppState,
    store,
};

pub fn init_router(s: AppState) -> Router {
    let protected = Router::new()
        .route("/consume", post(store::consume))
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

/// # Panics
///
/// Will panic if could not bind to specified port or port is malformed.
pub async fn start_http<F>(store: Arc<Store>, signal: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    let port = env::var("HTTP_PORT").unwrap_or_else(|_| "8000".to_string());
    let listener = match tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await {
        Ok(l) => l,
        Err(e) => panic!("Failed to start application: {e:?}"),
    };

    info!("Starting on port: {port}");

    let state = AppState::new(store);
    let r = init_router(state);

    if let Err(e) = axum::serve(
        listener,
        r.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(signal)
    .await
    {
        panic!("Failed to start application: {e:?}")
    }
}

#[cfg(test)]
pub mod test {
    use std::sync::Arc;

    use testcontainers_modules::testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner};
    use that_limit_cache::CacheConfig;
    use that_limit_core::{Command, Config, Store};
    use that_limit_test_utils::logger::init_test_logger;
    use tokio::sync::broadcast;

    use crate::state::AppState;

    /// Wrapper App to keep redis container alive
    /// for the whole duration of the test.
    pub struct TestApp {
        inner: AppState,
        cfg: Config,
        _redis_container: Arc<ContainerAsync<testcontainers_modules::redis::Redis>>,
    }

    impl TestApp {
        pub async fn new() -> Self {
            Self::with_config(Config::default()).await
        }

        pub async fn with_config(cfg: Config) -> Self {
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

            let redis = CacheConfig::new(host, port).connect().await;
            let store = Store::new(cfg.clone(), redis, shutdown_tx);

            Self {
                inner: AppState::new(store.clone()),
                cfg,
                _redis_container: rc,
            }
        }

        pub const fn app_state(&self) -> &AppState {
            &self.inner
        }

        pub const fn config(&self) -> &Config {
            &self.cfg
        }
    }
}
