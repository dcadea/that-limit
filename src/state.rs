use std::sync::Arc;

use axum::extract::FromRef;

use crate::{cfg::Config, integration::cache, store};

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<store::Store>,
    pub redis: cache::Redis,
}

impl AppState {
    pub async fn new(cfg: Config) -> Self {
        let cache_cfg = cache::Config::env().unwrap_or_default();
        Self {
            store: Arc::new(store::Store::new(cfg)),
            redis: cache_cfg.connect().await,
        }
    }
}

impl FromRef<AppState> for Arc<store::Store> {
    fn from_ref(s: &AppState) -> Self {
        s.store.clone()
    }
}

impl FromRef<AppState> for cache::Redis {
    fn from_ref(s: &AppState) -> Self {
        s.redis.clone()
    }
}
