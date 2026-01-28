use std::{
    cmp::min,
    sync::Arc,
    time::{Duration, SystemTime},
};

use dashmap::{
    DashMap,
    try_result::TryResult::{Absent, Locked, Present},
};
use log::{debug, error};
use tokio::sync::broadcast::Receiver;

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
    pub fn new(
        config: Arc<Config>,
        redis: cache::Redis,
        mut shutdown_rx: Receiver<()>,
    ) -> Arc<Self> {
        let buckets = DashMap::with_capacity(10000);

        let s = Arc::new(Self {
            buckets,
            config,
            redis: redis.clone(),
        });

        let s_clone = s.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));

            loop {
                tokio::select! {
                    _ = interval.tick() => {
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
                    },
                    _ = shutdown_rx.recv() => {
                        debug!("Stop cleanup task and return leased tokens");

                        if s_clone.buckets.is_empty() {
                            break;
                        }

                        let replenish: Vec<_> = s_clone
                            .buckets
                            .iter()
                            .filter(|e| e.value().tokens > 0)
                            .collect();

                        for e in replenish {
                            let key = cache::Key::from(e.key());
                            let tokens_left = e.value().tokens;
                            if let Err(e) = redis.incr(&key, tokens_left).await {
                                error!("Could not return left tokens for: {key:?}, {e:?}");
                            }
                        }

                        break;
                    }
                }
            }
        });

        s
    }

    pub async fn lease(&self, b_id: bucket::Id) -> Result<()> {
        let key = cache::Key::from(&b_id);

        let tokens: cache::Result<u64> = self.redis.get(&key).await;

        let (leased, ttl) = match tokens {
            // We'll respond with 429 when cannot lease from cache anymore
            Ok(0) => return Err(Error::Exhausted(b_id)),
            Ok(tokens) => {
                let leased = min(tokens, LEASE_SIZE);

                self.redis.decr(&key, leased).await?;

                let ttl = self.redis.ttl(&key).await?;
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
        debug!("Leased {leased} tokens for {b_id:?}");
        self.add(b_id, leased, ttl);
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
                    return Ok(false);
                }

                debug!("Tokens for {b_id} left: {}", b.tokens);
                Ok(true)
            }
            Absent => Ok(false),
            Locked => Err(Error::Locked(b_id.clone())),
        }
    }
}

pub mod handler {
    use std::sync::Arc;

    use axum::{Extension, Json, extract::State};
    use serde::{Deserialize, Serialize};

    use crate::{bucket, store::Store};

    #[derive(Serialize, Deserialize)]
    #[cfg_attr(test, derive(PartialEq, Eq, Debug))]
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

    #[derive(Serialize, Deserialize)]
    #[cfg_attr(test, derive(PartialEq, Eq, Debug))]
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

    #[cfg(test)]
    mod test {
        use axum::{
            body::Body,
            http::{self, Request, StatusCode},
        };
        use http_body_util::BodyExt;
        use tower::ServiceExt;

        use crate::{
            bucket, init_router,
            state::test::State,
            store::handler::{CheckResponse, ConsumeResponse, test},
        };

        #[tokio::test]
        async fn should_respond_ok_on_consume_authorized() {
            let ts = test::State::new().await;
            let app = init_router(ts.app_state().clone());

            let response = app
                .oneshot(
                    Request::builder()
                        .method(http::Method::POST)
                        .uri("/consume")
                        .header("user_id", "valera")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(StatusCode::OK, response.status());

            let expexted = ConsumeResponse {
                bucket_id: bucket::Id::Protected("valera".to_string()),
                tokens_left: 99,
            };

            let body = response.into_body().collect().await.unwrap().to_bytes();
            let actual: ConsumeResponse = serde_json::from_slice(&body).unwrap();

            assert_eq!(expexted, actual);
        }

        #[tokio::test]
        async fn should_respond_ok_on_subsequent_consume() {
            let ts = test::State::new().await;
            let app = init_router(ts.app_state().clone());

            // first call will lease
            let _ = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(http::Method::POST)
                        .uri("/consume")
                        .header("user_id", "valera")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            // second call will skip lease and deduct from local store
            let response = app
                .oneshot(
                    Request::builder()
                        .method(http::Method::POST)
                        .uri("/consume")
                        .header("user_id", "valera")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(StatusCode::OK, response.status());

            let expexted = ConsumeResponse {
                bucket_id: bucket::Id::Protected("valera".to_string()),
                tokens_left: 98,
            };

            let body = response.into_body().collect().await.unwrap().to_bytes();
            let actual: ConsumeResponse = serde_json::from_slice(&body).unwrap();

            assert_eq!(expexted, actual);
        }

        #[tokio::test]
        async fn should_fail_on_consume_unauthorized() {
            let ts = test::State::new().await;
            let app = init_router(ts.app_state().clone());

            let response = app
                .oneshot(
                    Request::builder()
                        .method(http::Method::POST)
                        .uri("/consume")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(StatusCode::UNAUTHORIZED, response.status());
        }

        #[tokio::test]
        async fn should_return_allowed_true_on_check() {
            let ts = test::State::new().await;
            let app = init_router(ts.app_state().clone());

            // ignore response, this is to lease first batch of tokens
            let _ = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(http::Method::POST)
                        .uri("/consume")
                        .header("user_id", "valera")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            let response = app
                .oneshot(
                    Request::builder()
                        .method(http::Method::GET)
                        .uri("/check")
                        .header("user_id", "valera")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(StatusCode::OK, response.status());

            let expexted = CheckResponse {
                bucket_id: bucket::Id::Protected("valera".to_string()),
                allowed: true,
            };

            let body = response.into_body().collect().await.unwrap().to_bytes();
            let actual: CheckResponse = serde_json::from_slice(&body).unwrap();

            assert_eq!(expexted, actual);
        }

        #[tokio::test]
        async fn should_return_allowed_false_on_check() {
            let ts = test::State::new().await;
            let app = init_router(ts.app_state().clone());

            let response = app
                .oneshot(
                    Request::builder()
                        .method(http::Method::GET)
                        .uri("/check")
                        .header("user_id", "valera")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(StatusCode::OK, response.status());

            let expexted = CheckResponse {
                bucket_id: bucket::Id::Protected("valera".to_string()),
                allowed: false,
            };

            let body = response.into_body().collect().await.unwrap().to_bytes();
            let actual: CheckResponse = serde_json::from_slice(&body).unwrap();

            assert_eq!(expexted, actual);
        }
    }
}
