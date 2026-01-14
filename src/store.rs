use std::{
    net::{IpAddr, Ipv4Addr},
    sync::Arc,
    time::{Duration, SystemTime},
};

use dashmap::DashMap;

use crate::{
    bucket::{self, Bucket},
    cfg::Config,
};

#[derive(Debug)]
pub enum Error {
    Exhausted(bucket::Id),
    NotFound(bucket::Id),
}

pub struct Store {
    store: DashMap<bucket::Id, Bucket>,
    config: Config,
}

impl Store {
    pub fn new(config: Config) -> Arc<Self> {
        let store = DashMap::with_capacity(10000);

        // TODO: remove
        store.insert(
            bucket::Id::Protected("jora".to_string()),
            Bucket::new(500, Duration::from_secs(3600)),
        );
        store.insert(
            bucket::Id::Public(IpAddr::V4(Ipv4Addr::new(10, 20, 30, 40))),
            Bucket::new(10000, Duration::from_secs(600)),
        );

        let s = Arc::new(Self { store, config });

        let s_clone = s.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));

            loop {
                tokio::select! {
                    // _ = async {
                    //     // TODO: gracefully shutdown
                    //     // if let Some(rx) = &mut shutdown {
                    //     //     let _ = rx.await;
                    //     // } else {
                    //     //     std::future::pending::<()>().await
                    //     // }
                    // } => {
                    //     println!("Cleanup task received shutdown signal, exiting...");
                    //     break;
                    // }

                    _ = interval.tick() => {
                        let now = SystemTime::now();
                        let expired: Vec<_> = s_clone.store
                            .iter()
                            .filter(|e| e.value().expires_at <= now)
                            .map(|e| e.key().clone())
                            .collect();

                        for key in expired {
                            s_clone.store.remove(&key);
                        }
                    }
                }
            }
        });

        s
    }

    pub const fn config(&self) -> &Config {
        &self.config
    }

    pub fn add(&self, b_id: bucket::Id, tokens: u128, ttl: Duration) {
        self.store.insert(b_id, Bucket::new(tokens, ttl));
    }

    pub fn consume(&self, b_id: &bucket::Id) -> Option<u128> {
        if let Some(mut b) = self.store.get_mut(b_id) {
            if b.expires_at <= SystemTime::now() {
                drop(b);
                self.store.remove(b_id);
                return None;
            }

            if b.tokens > 0 {
                b.tokens -= 1;
            }

            return Some(b.tokens);
        }

        None
    }

    pub fn check(&self, b_id: &bucket::Id) -> bool {
        self.store.contains_key(b_id)
    }

    pub fn get_tokens(&self, b_id: &bucket::Id) -> Result<u128, Error> {
        match self.store.get(b_id) {
            Some(b) => {
                if b.expires_at <= SystemTime::now() {
                    return Err(Error::Exhausted(b_id.clone()));
                }

                Ok(b.tokens)
            }
            None => Err(Error::NotFound(b_id.clone())),
        }
    }
}

pub mod handler {
    use std::sync::Arc;

    use axum::{Extension, extract::State, http::StatusCode, response::IntoResponse};

    use crate::{
        bucket,
        store::{Error, Store},
    };

    pub async fn consume(
        b_id: Extension<bucket::Id>,
        store: State<Arc<Store>>,
    ) -> impl IntoResponse {
        let tokens_left = store.consume(&b_id);

        let response = serde_json::json!({
            "bucket_id": b_id.0,
            "tokens_left": tokens_left.unwrap_or(0)
        });
        axum::Json(response)
    }

    pub async fn check(
        Extension(b_id): Extension<bucket::Id>,
        store: State<Arc<Store>>,
    ) -> impl IntoResponse {
        let t = store.get_tokens(&b_id);

        match t {
            Ok(t) => {
                let response = serde_json::json!({
                    "user_id": b_id,
                    "tokens_left": t
                });
                (StatusCode::OK, axum::Json(response))
            }
            Err(Error::NotFound(b_id)) => {
                let response = serde_json::json!({
                    "error": format!("User: {} not found in store", b_id)
                });
                (StatusCode::NOT_FOUND, axum::Json(response))
            }
            Err(Error::Exhausted(b_id)) => {
                let response = serde_json::json!({
                    "error": format!("User: {} consumed all tokens", b_id)
                });
                (StatusCode::TOO_MANY_REQUESTS, axum::Json(response))
            }
        }
    }
}
