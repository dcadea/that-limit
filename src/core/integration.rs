#[derive(Clone, Debug)]
pub enum Command {
    Shutdown,
    CleanupComplete,
}

pub mod cache {
    use std::{env, fmt::Debug};

    use log::warn;
    use redis::RedisError;

    use crate::core::bucket;

    pub type Result<T> = std::result::Result<T, Error>;

    #[derive(thiserror::Error, Debug)]
    pub enum Error {
        #[error("Redis error occurred: {0}")]
        Redis(#[from] RedisError),
        #[error("Key {0} does not exist")]
        KeyDoesNotExist(String),
        #[error("Key {0} does not have expiration")]
        NoExpiration(String),
        #[error("Unexpected redis error: {0}")]
        Unexpected(String),
        #[error("Key {0} not found")]
        NotFound(String),
    }

    impl redis::ToRedisArgs for bucket::Id {
        fn write_redis_args<W>(&self, out: &mut W)
        where
            W: ?Sized + redis::RedisWrite,
        {
            match self {
                Self::Public(ip) => format!("ip:{ip}"),
                Self::Protected(sub) => format!("sub:{sub}"),
            }
            .write_redis_args(out);
        }
    }

    impl redis::ToSingleRedisArg for bucket::Id {}

    #[derive(Clone)]
    pub struct Config {
        host: String,
        port: u16,
    }

    impl Default for Config {
        fn default() -> Self {
            warn!("Fallback to default REDIS config");
            Self {
                host: String::from("127.0.0.1"),
                port: 6379,
            }
        }
    }

    impl Config {
        pub fn env() -> Option<Self> {
            let host = env::var("REDIS_HOST").ok();
            let port = env::var("REDIS_PORT")
                .ok()
                .and_then(|p| p.parse::<u16>().ok());

            if let (Some(host), Some(port)) = (host, port) {
                Some(Self { host, port })
            } else {
                warn!("REDIS env is not configured");
                None
            }
        }

        pub async fn connect(&self) -> Redis {
            let client = match redis::Client::open(format!("redis://{}:{}", self.host, self.port)) {
                Ok(client) => client,
                Err(e) => panic!("Failed to connect to Redis: {e:?}"),
            };
            let con = match client.get_connection_manager().await {
                Ok(con) => con,
                Err(e) => panic!("Failed create Redis connection manager: {e}"),
            };

            Redis { con }
        }
    }

    #[cfg(test)]
    mod test {
        use super::*;

        impl Config {
            pub fn test(host: String, port: u16) -> Self {
                Self { host, port }
            }
        }
    }

    #[derive(Clone)]
    pub struct Redis {
        con: redis::aio::ConnectionManager,
    }

    impl Redis {
        pub async fn execute<A>(&self, action: A) -> Result<A::Output>
        where
            A: action::Action,
        {
            action.execute(&mut self.con.clone()).await
        }
    }

    pub mod action {
        use std::{fmt::Debug, time::Duration};

        use log::trace;
        use redis::AsyncCommands;

        pub trait Action {
            type Output;

            #[allow(async_fn_in_trait)]
            async fn execute(
                self,
                con: &mut redis::aio::ConnectionManager,
            ) -> super::Result<Self::Output>;
        }

        pub struct Incr<K> {
            key: K,
            delta: u64,
        }

        impl<K> Incr<K> {
            pub const fn new(key: K, delta: u64) -> Self {
                Self { key, delta }
            }
        }

        impl<K> Action for Incr<K>
        where
            K: redis::ToSingleRedisArg + Sync + Debug + ToString,
        {
            type Output = ();

            async fn execute(
                self,
                con: &mut redis::aio::ConnectionManager,
            ) -> super::Result<Self::Output> {
                let key = self.key;
                trace!("INCR -> {key:?}");
                con.incr::<_, _, u64>(&key, self.delta).await?;
                Ok(())
            }
        }

        pub struct Lease<K> {
            key: K,
            size: u64,
            quota: u64,
            ttl: Duration,
        }

        impl<K> Lease<K> {
            pub const fn new(key: K, size: u64, quota: u64, ttl: Duration) -> Self {
                Self {
                    key,
                    size,
                    quota,
                    ttl,
                }
            }
        }

        const LEASE_SCRIPT: &str = r"
            local key = KEYS[1]
            local lease_size = tonumber(ARGV[1])
            local quota = tonumber(ARGV[2])
            local default_ttl = tonumber(ARGV[3])

            local current = redis.call('GET', key)

            if not current then
                local remaining = quota - lease_size
                redis.call('SETEX', key, default_ttl, remaining)
                return {lease_size, default_ttl}
            end

            local tokens = tonumber(current)
            if tokens <= 0 then
                return {0, redis.call('TTL', key)}
            end

            local to_lease = math.min(tokens, lease_size)
            redis.call('DECRBY', key, to_lease)
            local ttl = redis.call('TTL', key)

            return {to_lease, ttl}
        ";

        impl<K> Action for Lease<K>
        where
            K: redis::ToSingleRedisArg + Sync + Debug + ToString,
        {
            type Output = (u64, Duration);

            async fn execute(
                self,
                con: &mut redis::aio::ConnectionManager,
            ) -> super::Result<Self::Output> {
                let key = self.key;
                trace!("LEASE -> {key:?}");

                let (leased, ttl_secs): (u64, u64) = redis::Script::new(LEASE_SCRIPT)
                    .key(&key)
                    .arg(self.size)
                    .arg(self.quota)
                    .arg(self.ttl.as_secs())
                    .invoke_async(con)
                    .await?;

                let ttl = if ttl_secs > 0 {
                    Duration::from_secs(ttl_secs)
                } else {
                    self.ttl
                };

                Ok((leased, ttl))
            }
        }

        #[cfg(test)]
        pub mod test {
            use log::error;
            use std::marker::PhantomData;

            use crate::core::integration::cache;

            use super::*;

            pub struct Get<K, R> {
                key: K,
                _r: PhantomData<R>,
            }

            impl<K, R> Get<K, R> {
                pub const fn new(key: K) -> Self {
                    Self {
                        key,
                        _r: PhantomData,
                    }
                }
            }

            impl<K, R> Action for Get<K, R>
            where
                K: redis::ToSingleRedisArg + Sync + Debug + ToString,
                R: redis::FromRedisValue,
            {
                type Output = R;

                async fn execute(
                    self,
                    con: &mut redis::aio::ConnectionManager,
                ) -> cache::Result<Self::Output> {
                    let key = self.key;
                    match con.get::<_, Option<Self::Output>>(&key).await {
                        Ok(value) => {
                            let status = if value.is_some() { "Hit" } else { "Miss" };
                            trace!("GET ({status}) -> {key:?}");

                            value.ok_or(cache::Error::NotFound(key.to_string()))
                        }
                        Err(e) => {
                            error!("Failed to GET on {key:?}. Reason: {e:?}");
                            Err(cache::Error::from(e))
                        }
                    }
                }
            }
        }
    }
}
