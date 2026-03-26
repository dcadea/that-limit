use std::{env, net::SocketAddr, sync::Arc};

use axum::{
    Router,
    http::StatusCode,
    middleware::from_fn,
    routing::{get, post},
};
use log::info;
use that_limit_core::Store;

use crate::{middleware, state::AppState, store};

pub fn init_router(s: AppState) -> Router {
    let protected = Router::new()
        .route("/consume", post(store::consume))
        .layer(from_fn(middleware::find_token_claims));

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

    let state = AppState::new(store);
    let r = init_router(state);

    info!("Starting on port: {port}");

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

    use testcontainers_modules::testcontainers::ContainerAsync;
    use that_limit_core::Config;
    use that_limit_test_utils::{config::ConfigExt, logger::init_test_logger, store::init_store};

    use crate::state::AppState;

    pub const TEST_DOMAIN: &str = "test-app";

    /// Wrapper App to keep redis container alive
    /// for the whole duration of the test.
    pub struct TestApp {
        inner: AppState,
        cfg: Config,
        _redis_container: Arc<ContainerAsync<testcontainers_modules::redis::Redis>>,
    }

    impl TestApp {
        pub async fn new() -> Self {
            Self::with_config(Config::default().with_domain(TEST_DOMAIN)).await
        }

        pub async fn with_config(cfg: Config) -> Self {
            init_test_logger();

            let (store, rc) = init_store(&cfg).await;

            Self {
                inner: AppState::new(store.clone()),
                cfg,
                _redis_container: Arc::new(rc),
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
