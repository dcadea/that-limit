use std::{env, sync::Arc};

use axum::extract::FromRef;
use tokio::sync::broadcast::Sender;

use crate::{
    cfg::{self},
    integration::cache,
    store,
};

#[derive(Clone)]
pub struct AppState {
    cfg: Arc<cfg::Config>,
    store: Arc<store::Store>,
}

impl AppState {
    pub async fn new(shutdown_tx: Sender<()>) -> crate::Result<Self> {
        let cfg_path = env::var("CFG_PATH").unwrap_or_else(|_| String::from("static/config.json"));
        let cfg = Arc::new(cfg::get(&cfg_path)?);
        let redis = cache::Config::env().unwrap_or_default().connect().await;

        Ok(Self {
            cfg: cfg.clone(),
            store: store::Store::new(cfg, redis, shutdown_tx.subscribe()),
        })
    }

    pub fn store(&self) -> Arc<store::Store> {
        self.store.clone()
    }
}

impl FromRef<AppState> for Arc<cfg::Config> {
    fn from_ref(s: &AppState) -> Self {
        s.cfg.clone()
    }
}

impl FromRef<AppState> for Arc<store::Store> {
    fn from_ref(s: &AppState) -> Self {
        s.store.clone()
    }
}

#[cfg(test)]
pub mod test {

    use testcontainers_modules::testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner};

    use crate::{cfg::Config, init_test_logger};

    use super::*;

    /// Wrapper around AppState to keep redis container alive
    /// for the whole duration of the test.
    pub struct State {
        inner: AppState,
        _redis_container: Arc<ContainerAsync<testcontainers_modules::redis::Redis>>,
    }

    impl State {
        pub async fn new() -> Self {
            Self::with_cfg(Config::test()).await
        }

        pub async fn with_cfg(cfg: Config) -> Self {
            init_test_logger();

            let cfg = Arc::new(cfg);

            let rc = testcontainers_modules::redis::Redis::default()
                .with_tag("7")
                .start()
                .await
                .map(Arc::new)
                .unwrap();
            let host = rc.get_host().await.unwrap().to_string();
            let port = rc.get_host_port_ipv4(6379).await.unwrap();

            let redis = cache::Config::test(host, port).connect().await;

            let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);

            Self {
                inner: AppState {
                    cfg: cfg.clone(),
                    store: store::Store::new(cfg, redis, shutdown_tx.subscribe()),
                },
                _redis_container: rc,
            }
        }

        pub const fn app_state(&self) -> &AppState {
            &self.inner
        }
    }
}
