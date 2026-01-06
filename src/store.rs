use dashmap::DashMap;

use crate::cfg::Config;

#[derive(Debug)]
pub struct Store {
    pub store: DashMap<String, u128>,
    config: Config,
}

impl Store {
    pub fn new(config: Config) -> Self {
        let store = DashMap::with_capacity(10000);

        store.insert("jora".to_string(), 100);
        store.insert("valera".to_string(), 500);

        Self {
            store: store,
            config,
        }
    }

    pub fn add_public(&self, s: &str) {
        self.store.insert(s.to_string(), self.config.public.quota);
    }

    pub fn add_protected(&self, s: &str) {
        self.store
            .insert(s.to_string(), self.config.protected.quota);
    }

    pub fn consume(&self, s: &str) {
        if let Some(mut b) = self.store.get_mut(s) {
            if *b > 0 {
                *b -= 1;
            }
        }
    }

    pub fn get_quota(&self, s: &str) -> Option<u128> {
        self.store.get(s).map(|b| *b)
    }
}

pub mod handler {
    use std::sync::Arc;

    use axum::{Extension, extract::State, response::IntoResponse};

    use crate::{middleware::UserId, store::Store};

    pub async fn consume(
        Extension(user_id): Extension<UserId>,
        store: State<Arc<Store>>,
    ) -> impl IntoResponse {
        store.consume(&user_id.0);

        let response = serde_json::json!({
            "user_id": user_id.0,
            "message": "consumed 1 unit"
        });
        axum::Json(response)
    }

    pub async fn check(
        Extension(user_id): Extension<UserId>,
        store: State<Arc<Store>>,
    ) -> impl IntoResponse {
        let q = store.get_quota(&user_id.0);

        match q {
            Some(quota) => {
                let response = serde_json::json!({
                    "user_id": user_id.0,
                    "quota_left": quota
                });
                axum::Json(response)
            }
            None => {
                let response = serde_json::json!({
                    "error": format!("User: {} not found in store", user_id.0)
                });
                axum::Json(response)
            }
        }
    }
}
