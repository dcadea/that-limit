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
use tokio::sync::broadcast::Sender;

use crate::{
    bucket::{self, Bucket},
    cfg::Config,
    integration::{Command, cache},
};
use futures::future::join_all;

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
const CHUNK_SIZE: usize = 200;
const MAX_CONCURRENCY: usize = 5;

impl Store {
    pub fn new(
        config: Arc<Config>,
        redis: cache::Redis,
        shutdown_tx: Sender<Command>,
    ) -> Arc<Self> {
        let buckets = DashMap::with_capacity(10000);
        let mut shutdown_rx = shutdown_tx.subscribe();

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
                    Ok(Command::Shutdown) = shutdown_rx.recv() => {
                        debug!("Stop cleanup task and return leased tokens");

                        if s_clone.buckets.is_empty() {
                            break;
                        }

                        let replenish: Vec<_> = s_clone
                            .buckets
                            .iter()
                            .filter(|e| e.tokens > 0)
                            .filter(|e| e.expires_at > SystemTime::now())
                            .map(|e| (e.key().clone(), e.value().tokens))
                            .collect();

                        if replenish.is_empty() {
                            break;
                        }

                        debug!("# of buckets that should return leased tokens: {}", replenish.len());


                        let chunks = replenish
                            .chunks(CHUNK_SIZE)
                            .map(|chunk| {
                                let redis = redis.clone();

                                async move {
                                    for (key, tokens_left) in chunk {
                                        let key = cache::Key::from(key);
                                        if let Err(e) = redis.incr(&key, *tokens_left).await {
                                            error!("Could not return left tokens for: {key:?}, {e:?}");
                                        }
                                    }
                                }

                            });

                        join_all(chunks).await;

                        break;
                    },
                    _ = interval.tick() => {
                        if s_clone.buckets.is_empty() {
                            continue;

                        }

                        debug!("Cleanup tick");

                        let now = SystemTime::now();

                        let expired_keys: Vec<_> = s_clone
                            .buckets
                            .iter()
                            .filter(|e| e.value().expires_at <= now)
                            .map(|e| e.key().clone())
                            .collect();

                        if expired_keys.is_empty() {
                            continue;
                        }

                        debug!("Found {} expired buckets to cleanup", expired_keys.len());

                        let chunks: Vec<Vec<_>> = expired_keys
                            .chunks(CHUNK_SIZE)
                            .map(|c| c.to_vec())
                            .collect();

                        let handles = chunks
                        .into_iter()
                        .take(MAX_CONCURRENCY)
                        .map(|chunk| {
                            let s_for_task = s_clone.clone();
                            async move {
                                for key in chunk {
                                    s_for_task.buckets.remove(&key);
                                }
                            }
                        });

                        join_all(handles).await;

                    },
                }
            }

            if let Err(e) = shutdown_tx.send(Command::CleanupComplete) {
                error!("Failed to send CleanupComplete event, {e:?}");
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
            bucket,
            cfg::Config,
            init_router,
            middleware::{FORWARDED, USER_ID, X_FORWARDED_FOR, X_REAL_IP},
            state::test::State,
            store::{
                LEASE_SIZE,
                handler::{CheckResponse, ConsumeResponse, test},
            },
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
                        .header(USER_ID, "valera")
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
        async fn should_respond_ok_on_consume_public() {
            let ts = test::State::new().await;
            let app = init_router(ts.app_state().clone());

            let ip_headers = [
                (FORWARDED, "89.28.75.89"),
                (X_FORWARDED_FOR, "28.75.89.89"),
                (X_REAL_IP, "89.75.28.89"),
                (FORWARDED, "2001:db8::1"),
                (X_FORWARDED_FOR, "fe80::1"),
                (X_REAL_IP, "::ffff:192.0.2.128"),
            ];

            for (h, ip) in ip_headers {
                let response = app
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method(http::Method::POST)
                            .uri("/consume")
                            .header(h, ip)
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();

                assert_eq!(StatusCode::OK, response.status());

                let expexted = ConsumeResponse {
                    bucket_id: bucket::Id::Public(ip.parse().unwrap()),
                    tokens_left: 99,
                };

                let body = response.into_body().collect().await.unwrap().to_bytes();
                let actual: ConsumeResponse = serde_json::from_slice(&body).unwrap();

                assert_eq!(expexted, actual);
            }
        }

        #[tokio::test]
        async fn should_fail_on_consume_public_malformed() {
            let ts = test::State::new().await;
            let app = init_router(ts.app_state().clone());

            let ip_headers = [
                (FORWARDED, ""),
                (X_FORWARDED_FOR, "1.2.3"),
                (X_REAL_IP, "256.1.1.1"),
                (FORWARDED, "1.2.3.4 "),
                (X_FORWARDED_FOR, "01.02.03.04"),
                (X_REAL_IP, "1..3.4"),
                (FORWARDED, ":"),
                (X_FORWARDED_FOR, ":::"),
                (X_REAL_IP, "2001::db8::1"),
                (FORWARDED, "2001:dg8::1"),
                (X_FORWARDED_FOR, "fe80::1%eth0"),
                (X_REAL_IP, "::ffff:999.1.1.1"),
                (FORWARDED, "12345::1"),
            ];

            for (h, ip) in ip_headers {
                let response = app
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method(http::Method::POST)
                            .uri("/consume")
                            .header(h, ip)
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();

                assert_eq!(StatusCode::UNAUTHORIZED, response.status());
            }
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
                        .header(USER_ID, "valera")
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
                        .header(USER_ID, "valera")
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
                        .header(USER_ID, "valera")
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
                        .header(USER_ID, "valera")
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
                        .header(USER_ID, "valera")
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

        #[tokio::test]
        async fn should_return_exhausted_when_no_quota_left() {
            const QUOTA: u64 = LEASE_SIZE + 1;
            let ts = test::State::with_cfg(Config::with_quota(QUOTA)).await;
            let app = init_router(ts.app_state().clone());

            for _ in [0; QUOTA as usize] {
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
            }

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

            assert_eq!(StatusCode::TOO_MANY_REQUESTS, response.status());
        }
    }
}
