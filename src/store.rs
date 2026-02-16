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
    integration::{
        Command,
        cache::{
            self,
            action::{Decr, Get, Incr, SetEx, Ttl},
        },
    },
};
use futures::future::join_all;

type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Bucket {0} is exhausted")]
    Exhausted(bucket::Id),
    #[error("Bucket {0} is locked, too many concurrent calls")]
    Locked(bucket::Id),
    #[error(transparent)]
    Cache(#[from] cache::Error),
}

pub struct Store {
    buckets: DashMap<bucket::Id, Bucket>,
    config: Config,
    redis: cache::Redis,
}

const CHUNK_SIZE: usize = 200;

impl Store {
    pub fn new(
        config: Config,
        redis: cache::Redis,
        shutdown_tx: Option<Sender<Command>>,
    ) -> Arc<Self> {
        let buckets = DashMap::with_capacity(10000);

        let s = Arc::new(Self {
            buckets,
            config,
            redis: redis.clone(),
        });

        let s_clone = s.clone();

        if let Some(shutdown_tx) = shutdown_tx {
            let mut shutdown_rx = shutdown_tx.subscribe();

            tokio::spawn(async move {
                let mut interval = tokio::time::interval(s_clone.config.cleanup.interval);

                loop {
                    tokio::select! {
                        Ok(Command::Shutdown) = shutdown_rx.recv() => {
                            debug!("Stop cleanup task and return leased tokens");

                            if s_clone.buckets.is_empty() {
                                break;
                            }

                            let now = SystemTime::now();
                            let replenish: Vec<_> = s_clone
                                .buckets
                                .iter()
                                .filter(|e| e.tokens > 0)
                                .filter(|e| e.expires_at > now)
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
                                            if let Err(e) = redis.execute(Incr::new(key.clone(), *tokens_left)).await {
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

                            let mut handles = Vec::new();
                            for chunk in expired_keys.chunks(CHUNK_SIZE) {
                                handles.push({
                                    let s_clone = s_clone.clone();
                                    async move {
                                        for key in chunk {
                                            s_clone.buckets.remove(key);
                                        }
                                    }
                                });
                            }

                            join_all(handles).await;
                        },
                    }
                }

                if let Err(e) = shutdown_tx.send(Command::CleanupComplete) {
                    error!("Failed to send CleanupComplete event, {e:?}");
                }
            });
        }

        s
    }

    pub async fn lease(&self, b_id: bucket::Id) -> Result<()> {
        let tokens: cache::Result<u64> = self.redis.execute(Get::new(b_id.clone())).await;

        let lease_size = self.config.lease_size;
        let (leased, ttl) = match tokens {
            // We'll respond with 429 when cannot lease from cache anymore
            Ok(0) => return Err(Error::Exhausted(b_id)),
            Ok(tokens) => {
                let leased = min(tokens, lease_size);

                self.redis.execute(Decr::new(b_id.clone(), leased)).await?;

                let ttl = self.redis.execute(Ttl::new(b_id.clone())).await?;
                (leased, ttl)
            }

            Err(cache::Error::NotFound(_)) => {
                let criteria = match b_id {
                    bucket::Id::Public(_) => &self.config.public,
                    bucket::Id::Protected(_) => &self.config.protected,
                };

                // ideally should never happen, but if will - panic
                assert!(criteria.quota() >= lease_size);

                let ttl = criteria.reset_in();

                self.redis
                    .execute(SetEx::new(b_id.clone(), criteria.quota() - lease_size, ttl))
                    .await?;

                (lease_size, ttl)
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

#[cfg(test)]
mod test {
    use std::{net::IpAddr, sync::Arc, time::Duration};

    use testcontainers_modules::testcontainers::{ImageExt, runners::AsyncRunner};
    use tokio::{sync::broadcast, time};

    use super::*;

    use crate::{
        bucket,
        cfg::{Cleanup, Config},
        integration::{Command, cache},
    };

    #[tokio::test]
    async fn should_perform_cleanup_on_tick() {
        let rc = testcontainers_modules::redis::Redis::default()
            .with_tag("7")
            .start()
            .await
            .map(Arc::new)
            .unwrap();
        let host = rc.get_host().await.unwrap().to_string();
        let port = rc.get_host_port_ipv4(6379).await.unwrap();
        let redis = cache::Config::test(host, port).connect().await;

        let cfg = Config::default()
            .with_cleanup(Cleanup {
                enabled: true,
                interval: Duration::from_millis(50),
            })
            .with_protected_reset_in(Duration::from_millis(25));

        let (shutdown_tx, _) = broadcast::channel::<Command>(10);

        let store = Store::new(cfg.clone(), redis.clone(), Some(shutdown_tx));

        let valera = bucket::Id::Protected("valera".to_string());
        let jora = bucket::Id::Protected("jora".to_string());
        let public = bucket::Id::Public("89.28.75.89".parse::<IpAddr>().unwrap());

        for b_id in [&valera, &jora, &public] {
            let tokens = cfg.quota(b_id);
            let ttl = cfg.reset_in(b_id);
            store.add(b_id.clone(), tokens, ttl);
        }

        // let cleanup task to tick
        time::sleep(Duration::from_millis(75)).await;

        assert!(!store.check(&valera).unwrap());
        assert!(!store.check(&jora).unwrap());
        assert!(store.check(&public).unwrap());
    }

    #[tokio::test]
    async fn should_perform_shutdown_on_signal() {
        let rc = testcontainers_modules::redis::Redis::default()
            .with_tag("7")
            .start()
            .await
            .map(Arc::new)
            .unwrap();
        let host = rc.get_host().await.unwrap().to_string();
        let port = rc.get_host_port_ipv4(6379).await.unwrap();
        let redis = cache::Config::test(host, port).connect().await;

        let cfg = Config::default().with_cleanup(Cleanup {
            enabled: true,
            interval: Duration::from_millis(25),
        });

        let (shutdown_tx, _) = broadcast::channel::<Command>(10);

        let tx_clone = shutdown_tx.clone();
        tokio::spawn(async move {
            let tx_clone = tx_clone.clone();
            time::sleep(Duration::from_millis(50)).await;
            tx_clone.send(Command::Shutdown).unwrap();
        });

        let store = Store::new(cfg.clone(), redis.clone(), Some(shutdown_tx.clone()));

        let valera = bucket::Id::Protected("valera".to_string());
        let jora = bucket::Id::Protected("jora".to_string());
        let public = bucket::Id::Public("89.28.75.89".parse::<IpAddr>().unwrap());

        for b_id in [&valera, &jora, &public] {
            let tokens = cfg.quota(b_id);
            let ttl = cfg.reset_in(b_id);
            store.add(b_id.clone(), tokens, ttl);
        }

        // wait for shutdown command
        time::sleep(Duration::from_millis(75)).await;

        // no cleanup was performed, all buckets should still have tokens left
        assert!(store.check(&valera).unwrap());
        assert!(store.check(&jora).unwrap());
        assert!(store.check(&public).unwrap());

        // tokens should return to redis
        assert_eq!(
            cfg.protected.quota(),
            redis.execute(Get::<_, u64>::new(valera)).await.unwrap()
        );
        assert_eq!(
            cfg.protected.quota(),
            redis.execute(Get::<_, u64>::new(jora)).await.unwrap()
        );
        assert_eq!(
            cfg.public.quota(),
            redis.execute(Get::<_, u64>::new(public)).await.unwrap()
        )
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
        use std::time::Duration;

        use axum::{
            body::Body,
            http::{self, Request, StatusCode},
        };
        use http_body_util::BodyExt;
        use tokio::time;
        use tower::ServiceExt;

        use crate::{
            bootstrap::{init_router, test::TestApp},
            bucket,
            cfg::Config,
            middleware::{FORWARDED, USER_ID, X_FORWARDED_FOR, X_REAL_IP},
            store::handler::{CheckResponse, ConsumeResponse},
        };

        const TEST_USER: &str = "valera";

        fn consume_request(user_id: &str) -> Request<Body> {
            Request::builder()
                .method(http::Method::POST)
                .uri("/consume")
                .header(USER_ID, user_id)
                .body(Body::empty())
                .unwrap()
        }

        fn check_request(user_id: &str) -> Request<Body> {
            Request::builder()
                .method(http::Method::GET)
                .uri("/check")
                .header(USER_ID, user_id)
                .body(Body::empty())
                .unwrap()
        }

        #[tokio::test]
        async fn should_respond_ok_on_consume_authorized() {
            let app = TestApp::new().await;
            let r = init_router(app.app_state().clone());

            let response = r.oneshot(consume_request(TEST_USER)).await.unwrap();

            assert_eq!(StatusCode::OK, response.status());

            let config = app.config();
            let expexted = ConsumeResponse {
                bucket_id: bucket::Id::Protected(TEST_USER.to_string()),
                tokens_left: config.lease_size - 1,
            };

            let body = response.into_body().collect().await.unwrap().to_bytes();
            let actual: ConsumeResponse = serde_json::from_slice(&body).unwrap();

            assert_eq!(expexted, actual);
        }

        #[tokio::test]
        async fn should_respond_ok_on_consume_public() {
            let app = TestApp::new().await;
            let r = init_router(app.app_state().clone());

            let ip_headers = [
                (FORWARDED, "89.28.75.89"),
                (X_FORWARDED_FOR, "28.75.89.89"),
                (X_REAL_IP, "89.75.28.89"),
                (FORWARDED, "2001:db8::1"),
                (X_FORWARDED_FOR, "fe80::1"),
                (X_REAL_IP, "::ffff:192.0.2.128"),
            ];

            for (h, ip) in ip_headers {
                let response = r
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

                let config = app.config();
                let expexted = ConsumeResponse {
                    bucket_id: bucket::Id::Public(ip.parse().unwrap()),
                    tokens_left: config.lease_size - 1,
                };

                let body = response.into_body().collect().await.unwrap().to_bytes();
                let actual: ConsumeResponse = serde_json::from_slice(&body).unwrap();

                assert_eq!(expexted, actual);
            }
        }

        #[tokio::test]
        async fn should_fail_on_consume_public_malformed() {
            let app = TestApp::new().await;
            let r = init_router(app.app_state().clone());

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
                let response = r
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
            let app = TestApp::new().await;
            let r = init_router(app.app_state().clone());

            // first call will lease
            let _ = r.clone().oneshot(consume_request(TEST_USER)).await.unwrap();

            // second call will skip lease and deduct from local store
            let response = r.oneshot(consume_request(TEST_USER)).await.unwrap();

            assert_eq!(StatusCode::OK, response.status());

            let config = app.config();
            let expexted = ConsumeResponse {
                bucket_id: bucket::Id::Protected(TEST_USER.to_string()),
                tokens_left: config.lease_size - 2,
            };

            let body = response.into_body().collect().await.unwrap().to_bytes();
            let actual: ConsumeResponse = serde_json::from_slice(&body).unwrap();

            assert_eq!(expexted, actual);
        }

        #[tokio::test]
        async fn should_fail_on_consume_unauthorized() {
            let app = TestApp::new().await;
            let r = init_router(app.app_state().clone());

            let response = r
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
            let app = TestApp::new().await;
            let r = init_router(app.app_state().clone());

            // ignore response, this is to lease first batch of tokens
            let _ = r.clone().oneshot(consume_request(TEST_USER)).await.unwrap();

            let response = r.oneshot(check_request(TEST_USER)).await.unwrap();

            assert_eq!(StatusCode::OK, response.status());

            let expexted = CheckResponse {
                bucket_id: bucket::Id::Protected(TEST_USER.to_string()),
                allowed: true,
            };

            let body = response.into_body().collect().await.unwrap().to_bytes();
            let actual: CheckResponse = serde_json::from_slice(&body).unwrap();

            assert_eq!(expexted, actual);
        }

        #[tokio::test]
        async fn should_return_allowed_false_on_check() {
            let app = TestApp::new().await;
            let r = init_router(app.app_state().clone());

            let response = r.oneshot(check_request(TEST_USER)).await.unwrap();

            assert_eq!(StatusCode::OK, response.status());

            let expexted = CheckResponse {
                bucket_id: bucket::Id::Protected(TEST_USER.to_string()),
                allowed: false,
            };

            let body = response.into_body().collect().await.unwrap().to_bytes();
            let actual: CheckResponse = serde_json::from_slice(&body).unwrap();

            assert_eq!(expexted, actual);
        }

        #[tokio::test]
        async fn should_return_exhausted_when_no_quota_left() {
            let config = Config::default().with_protected_quota(1).with_lease_size(1);
            let app = TestApp::with_cfg(config).await;
            let r = init_router(app.app_state().clone());

            let _ = r.clone().oneshot(consume_request(TEST_USER)).await.unwrap();

            let response = r.oneshot(consume_request(TEST_USER)).await.unwrap();

            assert_eq!(StatusCode::TOO_MANY_REQUESTS, response.status());
        }

        #[tokio::test]
        async fn should_lease_more_tokens_when_bucket_is_exhausted_and_quota_not_exceeded() {
            let config = Config::default().with_protected_quota(5).with_lease_size(2);
            let app = TestApp::with_cfg(config).await;
            let r = init_router(app.app_state().clone());

            // first request leases first batch of tokens
            let response = r.clone().oneshot(consume_request(TEST_USER)).await.unwrap();
            assert_eq!(StatusCode::OK, response.status());
            let body = response.into_body().collect().await.unwrap().to_bytes();
            let actual: ConsumeResponse = serde_json::from_slice(&body).unwrap();
            assert_eq!(1, actual.tokens_left);

            // this will consume last token
            let response = r.clone().oneshot(consume_request(TEST_USER)).await.unwrap();
            assert_eq!(StatusCode::OK, response.status());
            let body = response.into_body().collect().await.unwrap().to_bytes();
            let actual: ConsumeResponse = serde_json::from_slice(&body).unwrap();
            assert_eq!(0, actual.tokens_left);

            // one more request to lease new batch of tokens
            let response = r.oneshot(consume_request(TEST_USER)).await.unwrap();
            assert_eq!(StatusCode::OK, response.status());
            let body = response.into_body().collect().await.unwrap().to_bytes();
            let actual: ConsumeResponse = serde_json::from_slice(&body).unwrap();
            assert_eq!(1, actual.tokens_left);
        }

        #[tokio::test]
        async fn should_return_zero_when_bucket_is_expired() {
            let reset_in = Duration::from_secs(1);
            let config = Config::default().with_protected_reset_in(reset_in);
            let app = TestApp::with_cfg(config).await;
            let r = init_router(app.app_state().clone());

            // trigger initial lease with bucket lifetime of 1 second (min allowed by redis)
            let response = r.clone().oneshot(consume_request(TEST_USER)).await.unwrap();
            assert_eq!(StatusCode::OK, response.status());
            let body = response.into_body().collect().await.unwrap().to_bytes();
            let actual: ConsumeResponse = serde_json::from_slice(&body).unwrap();
            assert_eq!(app.config().lease_size - 1, actual.tokens_left);

            // wait for bucket to expire
            time::sleep(reset_in).await;

            let response = r.clone().oneshot(consume_request(TEST_USER)).await.unwrap();
            assert_eq!(StatusCode::OK, response.status());
            let body = response.into_body().collect().await.unwrap().to_bytes();
            let actual: ConsumeResponse = serde_json::from_slice(&body).unwrap();
            assert_eq!(0, actual.tokens_left);
        }
    }
}
