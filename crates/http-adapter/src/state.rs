use std::sync::Arc;

use axum::extract::FromRef;
use that_limit_core::Store;

#[derive(Clone)]
pub struct AppState {
    store: Arc<Store>,
}

impl AppState {
    pub const fn new(store: Arc<Store>) -> Self {
        Self { store }
    }
}

impl FromRef<AppState> for Arc<Store> {
    fn from_ref(s: &AppState) -> Self {
        s.store.clone()
    }
}
