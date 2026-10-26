use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use dashmap::DashMap;
use log::{debug, error};
use tokio::sync::{Notify, broadcast::Sender};

use crate::{
    bucket::{self, Bucket},
    cache,
    config::Config,
    integration::Command,
};
use futures::future::join_all;

type Result<T> = std::result::Result<T, Error>;

// TODO: add dedicated error for expired
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Bucket {0} is exhausted, retry after: {1:?}")]
    Exhausted(bucket::Id, Duration),
    #[error(transparent)]
    Cache(#[from] that_limit_cache::Error),
}

pub struct Store {
    buckets: DashMap<bucket::Id, Bucket, ahash::RandomState>,
    refills: DashMap<bucket::Id, Arc<Notify>, ahash::RandomState>,
    config: Config,
    redis: that_limit_cache::Redis,
}

const CHUNK_SIZE: usize = 2000;

impl Store {
    #[must_use]
    pub fn new(
        config: Config,
        redis: that_limit_cache::Redis,
        shutdown_tx: Option<Sender<Command>>,
    ) -> Arc<Self> {
        let buckets = DashMap::with_capacity_and_hasher_and_shard_amount(
            100_000,
            ahash::RandomState::new(),
            1024,
        );

        let refills = DashMap::with_capacity_and_hasher_and_shard_amount(
            1000,
            ahash::RandomState::new(),
            128,
        );

        let s = Arc::new(Self {
            buckets,
            refills,
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

                            let replenish: Vec<_> = s_clone
                                .buckets
                                .iter()
                                .filter(|e| !e.is_empty())
                                .filter(|e| !e.is_expired())
                                .map(|e| (e.key().clone(), e.value().tokens(), e.value().window()))
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
                                        for (key, tokens_left, window) in chunk {
                                            match cache::release(&redis, key, *tokens_left, *window).await {
                                                Ok(true) => {}
                                                Ok(false) => debug!("Window for {key:?} is gone, dropped {tokens_left} tokens"),
                                                Err(e) => error!("Could not return left tokens for: {key:?}, {e:?}"),
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

                            let expired_keys: Vec<_> = s_clone
                                .buckets
                                .iter()
                                .filter(|e| e.value().is_expired())
                                .map(|e| e.key().clone())
                                .collect();

                            if expired_keys.is_empty() {
                                continue;
                            }

                            debug!("Found {} expired buckets to cleanup", expired_keys.len());

                            let mut js = tokio::task::JoinSet::new();
                            for chunk in expired_keys.chunks(CHUNK_SIZE) {
                                let s_clone = s_clone.clone();
                                let chunk_owned = chunk.to_vec();

                                js.spawn(async move {
                                    for key in chunk_owned {
                                        s_clone.buckets.remove(&key);
                                    }
                                });
                            }

                            while js.join_next().await.is_some() {}
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
}

impl Store {
    #[must_use]
    pub fn should_handle(&self, domain: &String) -> bool {
        self.config.domains.contains(domain)
    }

    /// # Errors
    ///
    /// Will return `Err` if bucket associated with `b_id` is exhausted.
    pub async fn consume(&self, b_id: bucket::Id) -> Result<u64> {
        self.ensure_refilled(&b_id).await?;

        if let Some(b) = self.buckets.get(&b_id) {
            if b.is_expired() {
                debug!("Bucket {b_id} expired, cleaning up");
                drop(b);
                self.buckets.remove(&b_id);
                return Ok(0);
            }

            debug!("Consuming token from {b_id}");
            let tokens_left = b.consume();

            debug!("Tokens for {b_id} left: {tokens_left}");
            return Ok(tokens_left);
        }

        Ok(0)
    }
}

impl Store {
    async fn ensure_refilled(&self, b_id: &bucket::Id) -> Result<()> {
        loop {
            if self.check(b_id)? {
                return Ok(());
            }

            let notify = match self.refills.entry(b_id.clone()) {
                dashmap::Entry::Occupied(entry) => entry.get().clone(),
                dashmap::Entry::Vacant(entry) => {
                    let n = Arc::new(Notify::new());
                    entry.insert(n.clone());

                    let res = self.lease(b_id).await;

                    if let Some((_, n_to_notify)) = self.refills.remove(b_id) {
                        n_to_notify.notify_waiters();
                    }
                    return res;
                }
            };

            notify.notified().await;
        }
    }

    async fn lease(&self, b_id: &bucket::Id) -> Result<()> {
        let criteria = match b_id {
            bucket::Id::Public(_) => &self.config.public,
            bucket::Id::Protected(_) => &self.config.protected,
        };

        let (leased, ttl, window) = cache::lease(
            &self.redis,
            b_id,
            criteria.lease_size,
            criteria.quota,
            criteria.reset_in,
        )
        .await?;

        if leased == 0 {
            if let Some(b) = self.buckets.get(b_id) {
                b.set_exhausted();
                debug!("Marked {b_id:?} as exhausted");
                return Err(Error::Exhausted(
                    b_id.clone(),
                    b.expires_at().duration_since(Instant::now()),
                ));
            }

            debug!("ZERO tokens left in datasource for {b_id:?}, leased nothing");
            return Err(Error::Exhausted(b_id.clone(), ttl));
        }

        debug!("Leased {leased} tokens for {b_id:?}");
        self.buckets
            .insert(b_id.clone(), Bucket::new(leased, ttl, window));
        Ok(())
    }

    fn check(&self, b_id: &bucket::Id) -> Result<bool> {
        match self.buckets.get(b_id) {
            Some(b) => {
                if b.is_exhausted() {
                    debug!("Bucket {b_id} is exhausted");
                    return Err(Error::Exhausted(
                        b_id.clone(),
                        b.expires_at().duration_since(Instant::now()),
                    ));
                }

                if b.is_empty() {
                    return Ok(false);
                }

                if b.is_expired() {
                    return Ok(false);
                }

                debug!("Tokens for {b_id} left: {}", b.tokens());
                Ok(true)
            }
            None => Ok(false),
        }
    }
}

#[cfg(test)]
impl Store {
    fn add(&self, b_id: bucket::Id, tokens: u64, ttl: Duration) {
        self.buckets.insert(b_id, Bucket::new(tokens, ttl, 0));
    }
}

#[cfg(test)]
mod test {
    use std::{net::IpAddr, sync::Arc, time::Duration};

    use testcontainers_modules::testcontainers::{ImageExt, runners::AsyncRunner};
    use that_limit_test_utils::cache::get;
    use tokio::{sync::broadcast, time};

    use super::*;

    use crate::{
        bucket,
        config::{Cleanup, Config},
        integration::Command,
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
        let redis = that_limit_cache::CacheConfig::new(host, port)
            .connect()
            .await;

        let cfg = Config::default()
            .with_cleanup(Cleanup {
                enabled: true,
                interval: Duration::from_millis(50),
            })
            .with_protected_reset_in(Duration::from_millis(25));

        let (shutdown_tx, _) = broadcast::channel::<Command>(10);

        let store = Store::new(cfg.clone(), redis.clone(), Some(shutdown_tx));

        let valera = bucket::Id::Protected("valera".into());
        let jora = bucket::Id::Protected("jora".into());
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

    async fn start_redis() -> (
        that_limit_cache::Redis,
        testcontainers_modules::testcontainers::ContainerAsync<
            testcontainers_modules::redis::Redis,
        >,
    ) {
        let rc = testcontainers_modules::redis::Redis::default()
            .with_tag("7")
            .start()
            .await
            .unwrap();
        let host = rc.get_host().await.unwrap().to_string();
        let port = rc.get_host_port_ipv4(6379).await.unwrap();
        let redis = that_limit_cache::CacheConfig::new(host, port)
            .connect()
            .await;
        (redis, rc)
    }

    #[tokio::test]
    async fn should_perform_shutdown_on_signal() {
        let (redis, _rc) = start_redis().await;

        let cfg = Config::default().with_cleanup(Cleanup {
            enabled: true,
            interval: Duration::from_secs(60),
        });

        let (shutdown_tx, _) = broadcast::channel::<Command>(10);
        let store = Store::new(cfg.clone(), redis.clone(), Some(shutdown_tx.clone()));

        let valera = bucket::Id::Protected("valera".into());
        let public = bucket::Id::Public("89.28.75.89".parse::<IpAddr>().unwrap());

        // leases lease_size tokens and spends one
        store.consume(valera.clone()).await.unwrap();
        store.consume(public.clone()).await.unwrap();

        let mut rx = shutdown_tx.subscribe();
        shutdown_tx.send(Command::Shutdown).unwrap();
        while !matches!(rx.recv().await, Ok(Command::CleanupComplete)) {}

        // unspent tokens are back in redis
        assert_eq!(
            Some(cfg.protected.quota - 1),
            redis.eval(get(&valera)).await.unwrap()
        );
        assert_eq!(
            Some(cfg.public.quota - 1),
            redis.eval(get(&public)).await.unwrap()
        );
    }

    #[tokio::test]
    async fn should_not_release_into_expired_window() {
        let (redis, _rc) = start_redis().await;
        let valera = bucket::Id::Protected("valera".into());

        let (_, _, window) = cache::lease(&redis, &valera, 10, 100, Duration::from_secs(1))
            .await
            .unwrap();
        time::sleep(Duration::from_millis(1100)).await;

        let released = cache::release(&redis, &valera, 10, window).await.unwrap();

        assert!(!released);
        // key is not recreated without TTL
        assert_eq!(None, redis.eval(get(&valera)).await.unwrap());
    }

    #[tokio::test]
    async fn should_not_release_into_newer_window() {
        let (redis, _rc) = start_redis().await;
        let valera = bucket::Id::Protected("valera".into());

        let (_, _, old_window) = cache::lease(&redis, &valera, 10, 100, Duration::from_secs(1))
            .await
            .unwrap();
        time::sleep(Duration::from_millis(1100)).await;
        // another instance opens the next window
        cache::lease(&redis, &valera, 10, 100, Duration::from_secs(60))
            .await
            .unwrap();

        let released = cache::release(&redis, &valera, 10, old_window)
            .await
            .unwrap();

        assert!(!released);
        assert_eq!(Some(90), redis.eval(get(&valera)).await.unwrap());
    }
}
