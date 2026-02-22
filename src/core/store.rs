use std::{cmp::min, collections::HashMap, sync::Arc, time::Duration};

use dashmap::DashMap;
use log::{debug, error};
use tokio::sync::{Mutex, Notify, broadcast::Sender};

use crate::core::{
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
    #[error(transparent)]
    Cache(#[from] cache::Error),
}

pub struct Store {
    buckets: DashMap<bucket::Id, Bucket>,
    refills: Mutex<HashMap<bucket::Id, Arc<Notify>>>,
    config: Config,
    redis: cache::Redis,
}

const CHUNK_SIZE: usize = 2000;

impl Store {
    pub fn new(
        config: Config,
        redis: cache::Redis,
        shutdown_tx: Option<Sender<Command>>,
    ) -> Arc<Self> {
        let buckets = DashMap::with_capacity(10000);

        let s = Arc::new(Self {
            buckets,
            refills: Mutex::default(),
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
                                .map(|e| (e.key().clone(), e.value().tokens()))
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
}

impl Store {
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

            let notify = {
                let mut rg = self.refills.lock().await;

                if let Some(notify) = rg.get(b_id) {
                    notify.clone()
                } else {
                    let notify = Arc::new(Notify::new());
                    rg.insert(b_id.clone(), notify.clone());
                    drop(rg);

                    let result = self.lease(b_id.clone()).await;

                    let mut rg = self.refills.lock().await;
                    if rg.remove(b_id).is_some() {
                        notify.notify_waiters();
                    }
                    drop(rg);

                    return result;
                }
            };

            notify.notified().await;
        }
    }

    async fn lease(&self, b_id: bucket::Id) -> Result<()> {
        let tokens: cache::Result<u64> = self.redis.execute(Get::new(b_id.clone())).await;

        let criteria = match b_id {
            bucket::Id::Public(_) => &self.config.public,
            bucket::Id::Protected(_) => &self.config.protected,
        };

        let lease_size = criteria.lease_size();
        let (leased, ttl) = match tokens {
            Ok(0) => {
                // At this point redis bucket is also exhausted
                // Explicitly mark local bucket as exhausted to avoid unnecessary round trip
                self.mark_as_exhausted(&b_id);
                return Err(Error::Exhausted(b_id));
            }
            Ok(tokens) => {
                let leased = min(tokens, lease_size);

                self.redis.execute(Decr::new(b_id.clone(), leased)).await?;

                let ttl = self.redis.execute(Ttl::new(b_id.clone())).await?;
                (leased, ttl)
            }

            Err(cache::Error::NotFound(_)) => {
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

    fn add(&self, b_id: bucket::Id, tokens: u64, ttl: Duration) {
        self.buckets.insert(b_id, Bucket::new(tokens, ttl));
    }

    fn check(&self, b_id: &bucket::Id) -> Result<bool> {
        match self.buckets.get(b_id) {
            Some(b) => {
                if b.is_exhausted() {
                    debug!("Bucket {b_id} is exhausted");
                    return Err(Error::Exhausted(b_id.clone()));
                }

                if b.is_empty() {
                    return Ok(false);
                }

                debug!("Tokens for {b_id} left: {}", b.tokens());
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn mark_as_exhausted(&self, b_id: &bucket::Id) {
        if let Some(b) = self.buckets.get(b_id) {
            b.set_exhausted();
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
        core::bucket,
        core::cfg::{Cleanup, Config},
        core::integration::{Command, cache},
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
