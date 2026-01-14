use std::{env, sync::Arc};

use axum::extract::FromRef;

use crate::{
    cfg::{self},
    integration::cache,
    store,
};

#[derive(Clone)]
pub struct AppState {
    cfg: Arc<cfg::Config>,
    store: Arc<store::Store>,
    redis: cache::Redis,
}

impl AppState {
    pub async fn new() -> crate::Result<Self> {
        let cfg_path = env::var("CFG_PATH").unwrap_or_else(|_| String::from("static/config.json"));
        let cfg = Arc::new(cfg::get(&cfg_path)?);
        let cache_cfg = cache::Config::env().unwrap_or_default();

        Ok(Self {
            cfg: cfg.clone(),
            store: store::Store::new(cfg),
            redis: cache_cfg.connect().await,
        })
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

impl FromRef<AppState> for cache::Redis {
    fn from_ref(s: &AppState) -> Self {
        s.redis.clone()
    }
}
