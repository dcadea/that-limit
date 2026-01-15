use std::{
    net::{IpAddr, Ipv4Addr},
    sync::Arc,
    time::{Duration, SystemTime},
};

use dashmap::{
    DashMap,
    try_result::TryResult::{Absent, Locked, Present},
};
use log::debug;

use crate::{
    bucket::{self, Bucket},
    cfg::Config,
};

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Exhausted(bucket::Id),
    NotFound(bucket::Id),
    Locked(bucket::Id),
}

pub struct Store {
    store: DashMap<bucket::Id, Bucket>,
    config: Arc<Config>,
}

impl Store {
    pub fn new(config: Arc<Config>) -> Arc<Self> {
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
                // TODO: gracefully shutdown
                interval.tick().await;

                let now = SystemTime::now();
                let expired: Vec<_> = s_clone
                    .store
                    .iter()
                    .filter(|e| e.value().expires_at <= now)
                    .map(|e| e.key().clone())
                    .collect();

                for key in expired {
                    s_clone.store.remove(&key);
                }
            }
        });

        s
    }

    pub fn config(&self) -> Arc<Config> {
        self.config.clone()
    }

    pub fn add(&self, b_id: bucket::Id, tokens: u128, ttl: Duration) {
        self.store.insert(b_id, Bucket::new(tokens, ttl));
    }

    pub fn consume(&self, b_id: &bucket::Id) -> Result<u128> {
        match self.store.try_get_mut(b_id) {
            Present(mut b) => {
                if b.expires_at <= SystemTime::now() {
                    debug!("Bucket {b_id} expired, cleaning up");
                    drop(b);
                    self.store.remove(b_id);
                    return Ok(0);
                }

                if b.tokens > 0 {
                    debug!("Consuming token from {b_id}");
                    b.tokens -= 1;
                }

                debug!("Tokens for {b_id} left: {}", b.tokens);
                return Ok(b.tokens);
            }
            Absent => Ok(0),
            Locked => Err(Error::Locked(b_id.clone())),
        }
    }

    pub fn check(&self, b_id: &bucket::Id) -> Result<bool> {
        match self.store.try_get(b_id) {
            Present(b) => {
                if b.tokens <= 0 {
                    debug!("Bucket {b_id} is exhausted");
                    return Err(Error::Exhausted(b_id.clone()));
                }

                debug!("Tokens for {b_id} left: {}", b.tokens);
                Ok(true)
            }
            Absent => Err(Error::NotFound(b_id.clone())),
            Locked => Err(Error::Locked(b_id.clone())),
        }
    }
}

pub mod handler {
    use std::sync::Arc;

    use axum::{Extension, Json, extract::State};
    use serde::Serialize;

    use crate::{bucket, store::Store};

    #[derive(Serialize)]
    pub struct ConsumeResponse {
        bucket_id: bucket::Id,
        tokens_left: u64,
    }

    pub async fn consume(
        Extension(bucket_id): Extension<bucket::Id>,
        store: State<Arc<Store>>,
    ) -> crate::Result<Json<ConsumeResponse>> {
        let tokens_left = store.consume(&bucket_id)?;

        Ok(Json(ConsumeResponse {
            bucket_id,
            tokens_left,
        }))
    }

    #[derive(Serialize)]
    pub struct CheckResponse {
        bucket_id: bucket::Id,
        allowed: bool,
    }

    pub async fn check(
        Extension(bucket_id): Extension<bucket::Id>,
        store: State<Arc<Store>>,
    ) -> crate::Result<Json<CheckResponse>> {
        let allowed = store.check(&bucket_id)?;

        Ok(Json(CheckResponse { bucket_id, allowed }))
    }
}
