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
        use std::{fmt::Debug, marker::PhantomData, time::Duration};

        use log::{error, trace, warn};
        use redis::AsyncCommands;

        pub trait Action {
            type Output;

            #[allow(async_fn_in_trait)]
            async fn execute(
                self,
                con: &mut redis::aio::ConnectionManager,
            ) -> super::Result<Self::Output>;
        }

        pub struct SetEx<K, V> {
            key: K,
            value: V,
            ttl: Duration,
        }

        pub struct Get<K, R> {
            key: K,
            _r: PhantomData<R>,
        }

        pub struct Ttl<K> {
            key: K,
        }

        pub struct Incr<K> {
            key: K,
            delta: u64,
        }

        pub struct Decr<K> {
            key: K,
            delta: u64,
        }

        impl<K, V> SetEx<K, V> {
            pub const fn new(key: K, value: V, ttl: Duration) -> Self {
                Self { key, value, ttl }
            }
        }

        impl<K, R> Get<K, R> {
            pub const fn new(key: K) -> Self {
                Self {
                    key,
                    _r: PhantomData,
                }
            }
        }

        impl<K> Ttl<K> {
            pub const fn new(key: K) -> Self {
                Self { key }
            }
        }

        impl<K> Incr<K> {
            pub const fn new(key: K, delta: u64) -> Self {
                Self { key, delta }
            }
        }

        impl<K> Decr<K> {
            pub const fn new(key: K, delta: u64) -> Self {
                Self { key, delta }
            }
        }

        impl<K, V> Action for SetEx<K, V>
        where
            K: redis::ToSingleRedisArg + Sync + Debug,
            V: redis::ToSingleRedisArg + Send + Sync,
        {
            type Output = ();

            async fn execute(
                self,
                con: &mut redis::aio::ConnectionManager,
            ) -> super::Result<Self::Output> {
                let key = self.key;
                trace!("SET_EX -> {key:?}");
                con.set_ex::<_, _, ()>(&key, self.value, self.ttl.as_secs())
                    .await?;
                Ok(())
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
            ) -> super::Result<Self::Output> {
                let key = self.key;
                match con.get::<_, Option<Self::Output>>(&key).await {
                    Ok(value) => {
                        let status = if value.is_some() { "Hit" } else { "Miss" };
                        trace!("GET ({status}) -> {key:?}");

                        value.ok_or(super::Error::NotFound(key.to_string()))
                    }
                    Err(e) => {
                        error!("Failed to GET on {key:?}. Reason: {e:?}");
                        Err(super::Error::from(e))
                    }
                }
            }
        }

        impl<K> Action for Ttl<K>
        where
            K: redis::ToSingleRedisArg + Sync + Debug + ToString,
        {
            type Output = Duration;

            async fn execute(
                self,
                con: &mut redis::aio::ConnectionManager,
            ) -> super::Result<Self::Output> {
                let key = self.key;
                trace!("TTL -> {key:?}");
                match con.ttl::<_, i64>(&key).await {
                    Ok(ttl) if ttl > 0 => Ok(Duration::from_secs(ttl.cast_unsigned())),
                    Ok(v) => {
                        if v == -1 {
                            warn!("Key {key:?} has no expiration");
                            Err(super::Error::NoExpiration(key.to_string()))
                        } else if v == -2 {
                            warn!("Key {key:?} does not exist");
                            Err(super::Error::KeyDoesNotExist(key.to_string()))
                        } else {
                            error!("Invalid TTL: {v} for key {key:?}");
                            Err(super::Error::Unexpected("Invalid TTL value".to_string()))
                        }
                    }
                    Err(e) => Err(super::Error::from(e)),
                }
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

        impl<K> Action for Decr<K>
        where
            K: redis::ToSingleRedisArg + Sync + Debug + ToString,
        {
            type Output = ();

            async fn execute(
                self,
                con: &mut redis::aio::ConnectionManager,
            ) -> super::Result<Self::Output> {
                let key = self.key;
                trace!("DECR -> {key:?}");
                con.decr::<_, _, u64>(&key, self.delta).await?;
                Ok(())
            }
        }
    }
}
