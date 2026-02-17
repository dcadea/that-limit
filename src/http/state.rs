use std::sync::Arc;

use axum::extract::FromRef;

use crate::core::store;

#[derive(Clone)]
pub struct AppState {
    store: Arc<store::Store>,
}

impl AppState {
    pub const fn new(store: Arc<store::Store>) -> Self {
        Self { store }
    }
}

impl FromRef<AppState> for Arc<store::Store> {
    fn from_ref(s: &AppState) -> Self {
        s.store.clone()
    }
}
