use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};

use dashmap::DashMap;

use crate::{bucket::Bucket, cfg::Config};

pub enum Error {
    Bucket(SystemTimeError),
    Exhausted(String),
    NotFound(String),
}

impl From<SystemTimeError> for Error {
    fn from(e: SystemTimeError) -> Self {
        Self::Bucket(e)
    }
}

#[derive(Debug)]
pub struct Store {
    pub store: DashMap<String, Bucket>,
    config: Config,
}

impl Store {
    pub fn new(config: Config) -> Self {
        let store = DashMap::with_capacity(10000);

        // TODO: remove
        store.insert("jora".to_string(), Bucket::new(&config.public).unwrap());
        store.insert(
            "valera".to_string(),
            Bucket::new(&config.protected).unwrap(),
        );

        // TODO: perform cleanup every 5s
        // tokio::spawn(|| {})

        Self { store, config }
    }

    pub fn add_public(&self, s: &str) -> Result<(), Error> {
        self.store
            .insert(s.to_string(), Bucket::new(&self.config.public)?);

        Ok(())
    }

    pub fn add_protected(&self, s: &str) -> Result<(), Error> {
        self.store
            .insert(s.to_string(), Bucket::new(&self.config.protected)?);

        Ok(())
    }

    pub fn consume(&self, s: &str) -> Option<u128> {
        if let Some(mut b) = self.store.get_mut(s) {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u128;

            if b.expires_at <= now {
                return None;
            }

            if b.tokens > 0 {
                b.tokens -= 1;
            }

            return Some(b.tokens);
        }

        None
    }

    pub fn get_tokens(&self, s: &str) -> Result<u128, Error> {
        match self.store.get(s) {
            Some(b) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u128;

                if b.expires_at <= now {
                    return Err(Error::Exhausted(s.to_string()));
                }

                Ok(b.tokens)
            }
            None => Err(Error::NotFound(s.to_string())),
        }
    }
}

pub mod handler {
    use std::sync::Arc;

    use axum::{Extension, extract::State, http::StatusCode, response::IntoResponse};

    use crate::{
        middleware::UserId,
        store::{Error, Store},
    };

    pub async fn consume(
        Extension(user_id): Extension<UserId>,
        store: State<Arc<Store>>,
    ) -> impl IntoResponse {
        let tokens_left = store.consume(&user_id.0);

        let response = serde_json::json!({
            "user_id": user_id.0,
            "tokens_left": tokens_left.unwrap_or(0)
        });
        axum::Json(response)
    }

    pub async fn check(
        Extension(user_id): Extension<UserId>,
        store: State<Arc<Store>>,
    ) -> impl IntoResponse {
        let t = store.get_tokens(&user_id.0);

        match t {
            Ok(t) => {
                let response = serde_json::json!({
                    "user_id": user_id.0,
                    "tokens_left": t
                });
                (StatusCode::OK, axum::Json(response))
            }
            Err(Error::NotFound(user_id)) => {
                let response = serde_json::json!({
                    "error": format!("User: {} not found in store", user_id)
                });
                (StatusCode::NOT_FOUND, axum::Json(response))
            }
            Err(Error::Exhausted(user_id)) => {
                let response = serde_json::json!({
                    "error": format!("User: {} consumed all tokens", user_id)
                });
                (StatusCode::TOO_MANY_REQUESTS, axum::Json(response))
            }
            Err(_) => {
                let response = serde_json::json!({
                    "error": "Internal server error",
                });
                (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(response))
            }
        }
    }
}
