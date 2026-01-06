use std::sync::Arc;

use axum::extract::FromRef;

use crate::{
    cfg::{self, Config},
    store,
};

#[derive(Clone, Debug)]
pub struct AppState {
    pub cfg: Arc<cfg::service::Service>,
    pub store: Arc<store::Store>,
}

impl AppState {
    pub fn new(cfg: Config) -> Self {
        Self {
            cfg: Arc::new(cfg::service::Service::new()),
            store: Arc::new(store::Store::new(cfg)),
        }
    }
}

impl FromRef<AppState> for Arc<cfg::service::Service> {
    fn from_ref(input: &AppState) -> Self {
        input.cfg.clone()
    }
}

impl FromRef<AppState> for Arc<store::Store> {
    fn from_ref(input: &AppState) -> Self {
        input.store.clone()
    }
}
