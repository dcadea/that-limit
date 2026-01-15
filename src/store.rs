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
    integration::cache,
};

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Exhausted(bucket::Id),
    NotFound(bucket::Id),
    Locked(bucket::Id),
    Cache(cache::Error),
}

impl From<cache::Error> for Error {
    fn from(e: cache::Error) -> Self {
        Self::Cache(e)
    }
}

pub struct Store {
    buckets: DashMap<bucket::Id, Bucket>,
    config: Arc<Config>,
    redis: cache::Redis,
}

// TODO: make configurable based on number of nodes to not exceed bucket size
const LEASE_SIZE: u64 = 100;

impl Store {
    pub fn new(config: Arc<Config>, redis: cache::Redis) -> Arc<Self> {
        let buckets = DashMap::with_capacity(10000);

        // TODO: remove
        buckets.insert(
            bucket::Id::Protected("jora".to_string()),
            Bucket::new(500, Duration::from_secs(3600)),
        );
        buckets.insert(
            bucket::Id::Public(IpAddr::V4(Ipv4Addr::new(10, 20, 30, 40))),
            Bucket::new(10000, Duration::from_secs(600)),
        );

        let s = Arc::new(Self {
            buckets,
            config,
            redis,
        });

        let s_clone = s.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));

            loop {
                // TODO: gracefully shutdown
                interval.tick().await;

                let now = SystemTime::now();
                let expired: Vec<_> = s_clone
                    .buckets
                    .iter()
                    .filter(|e| e.value().expires_at <= now)
                    .map(|e| e.key().clone())
                    .collect();

                for key in expired {
                    s_clone.buckets.remove(&key);
                }
            }
        });

        s
    }

    pub async fn lease(&self, b_id: &bucket::Id) -> Result<()> {
        let key = cache::Key::from(b_id);

        let tokens: cache::Result<u64> = self.redis.get(&key).await;

        let (leased, ttl) = match tokens {
            Ok(0) => return Err(Error::Exhausted(b_id.clone())),
            Ok(tokens) => {
                // calculate how many tokens are leased
                // and how many tokens are left in bank afterwards
                let (leased, bank) = if tokens >= LEASE_SIZE {
                    (LEASE_SIZE, tokens - LEASE_SIZE)
                } else {
                    (tokens, 0)
                };

                let ttl = self.redis.ttl(&key).await?;
                self.redis.set_keep_ttl(&key, bank).await?;
                (leased, ttl)
            }

            Err(cache::Error::NotFound(_)) => {
                let criteria = match b_id {
                    bucket::Id::Public(_) => &self.config.public,
                    bucket::Id::Protected(_) => &self.config.protected,
                };

                // ideally should never happen, but if will - panic
                assert!(criteria.quota() > LEASE_SIZE);

                let ttl = criteria.reset_in();

                self.redis
                    .set_ex(&key, criteria.quota() - LEASE_SIZE, ttl)
                    .await?;

                (LEASE_SIZE, ttl)
            }

            Err(e) => return Err(Error::from(e)),
        };

        self.add(b_id.clone(), leased, ttl);

        Ok(())
    }

    pub fn add(&self, b_id: bucket::Id, tokens: u64, ttl: Duration) {
        self.buckets.insert(b_id, Bucket::new(tokens, ttl));
    }

    fn consume(&self, b_id: &bucket::Id) -> Result<u64> {
        match self.buckets.try_get_mut(b_id) {
            Present(mut b) => {
                if b.expires_at <= SystemTime::now() {
                    debug!("Bucket {b_id} expired, cleaning up");
                    drop(b);
                    self.buckets.remove(b_id);
                    return Ok(0);
                }

                if b.tokens > 0 {
                    debug!("Consuming token from {b_id}");
                    b.tokens -= 1;
                }

                debug!("Tokens for {b_id} left: {}", b.tokens);
                Ok(b.tokens)
            }
            Absent => Ok(0),
            Locked => Err(Error::Locked(b_id.clone())),
        }
    }

    pub fn check(&self, b_id: &bucket::Id) -> Result<bool> {
        match self.buckets.try_get(b_id) {
            Present(b) => {
                if b.tokens == 0 {
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

    use axum::{Extension, extract::State, http::StatusCode, response::IntoResponse};

    use crate::{bucket, store::Store};

    pub async fn consume(
        b_id: Extension<bucket::Id>,
        store: State<Arc<Store>>,
    ) -> crate::Result<impl IntoResponse> {
        let tokens_left = store.consume(&b_id)?;

        let response = serde_json::json!({
            "bucket_id": b_id.0,
            "tokens_left": tokens_left
        });
        Ok(axum::Json(response))
    }

    pub async fn check(
        Extension(b_id): Extension<bucket::Id>,
        store: State<Arc<Store>>,
    ) -> crate::Result<impl IntoResponse> {
        let allowed = store.check(&b_id)?;
        let response = serde_json::json!({
            "bucket_id": b_id,
            "allowed": allowed
        });
        Ok((StatusCode::OK, axum::Json(response)))
    }
}
