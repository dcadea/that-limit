use std::sync::Arc;

use axum::extract::FromRef;

use crate::cfg;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<cfg::service::Service>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            cfg: Arc::new(cfg::service::Service::new()),
        }
    }
}

impl FromRef<AppState> for Arc<cfg::service::Service> {
    fn from_ref(input: &AppState) -> Self {
        input.cfg.clone()
    }
}
