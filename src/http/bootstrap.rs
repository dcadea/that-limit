use std::{env, net::SocketAddr};

use axum::{
    Router,
    http::StatusCode,
    middleware::{from_fn, from_fn_with_state},
    routing::{get, post},
};
use log::info;
use tower::ServiceBuilder;

use crate::{
    bootstrap::App,
    http::{
        middleware::{extract_identifier, extract_ip, lease_tokens},
        state, store,
    },
};

pub fn init_router(s: state::AppState) -> Router {
    let protected = Router::new()
        .route(
            "/consume",
            post(store::consume).layer(from_fn_with_state(s.clone(), lease_tokens)),
        )
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

impl App {
    pub async fn run_http<F>(&self, shutdown_signal: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let port = env::var("HTTP_PORT").unwrap_or_else(|_| "8000".to_string());
        let listener = match tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await {
            Ok(l) => l,
            Err(e) => panic!("Failed to start application: {e:?}"),
        };

        info!("Starting on port: {port}");

        let state = state::AppState::new(self.store());
        let r = init_router(state);

        if let Err(e) = axum::serve(
            listener,
            r.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown_signal)
        .await
        {
            panic!("Failed to start application: {e:?}")
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
        core::{
            cfg::Config,
            integration::{Command, cache},
            store,
        },
        http::state::AppState,
    };

    /// Wrapper App to keep redis container alive
    /// for the whole duration of the test.
    pub struct TestApp {
        inner: AppState,
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
