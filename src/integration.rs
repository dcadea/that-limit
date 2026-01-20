pub mod cache {
    use std::{
        env,
        fmt::{Display, Formatter},
        net::IpAddr,
        time::Duration,
    };

    use log::{error, trace, warn};
    use redis::{AsyncCommands, RedisError, SetOptions};

    use crate::bucket;

    pub type Result<T> = std::result::Result<T, Error>;

    #[derive(Debug)]
    pub enum Error {
        Redis(RedisError),
        KeyDoesNotExist(String),
        NoExpiration(String),
        Unexpected(String),
        NotFound(String),
    }

    impl From<RedisError> for Error {
        fn from(e: RedisError) -> Self {
            Self::Redis(e)
        }
    }

    #[derive(Clone, Debug)]
    pub enum Key<'a> {
        Sub(&'a str),
        Ip(IpAddr),
    }

    impl Display for Key<'_> {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Sub(sub) => write!(f, "sub:{sub}"),
                Self::Ip(ip) => write!(f, "ip:{ip}"),
            }
        }
    }

    impl redis::ToRedisArgs for Key<'_> {
        fn write_redis_args<W>(&self, out: &mut W)
        where
            W: ?Sized + redis::RedisWrite,
        {
            self.to_string().write_redis_args(out);
        }
    }

    impl redis::ToSingleRedisArg for Key<'_> {}

    impl<'a> From<&'a bucket::Id> for Key<'a> {
        fn from(id: &'a bucket::Id) -> Self {
            match id {
                bucket::Id::Public(ip) => Self::Ip(*ip),
                bucket::Id::Protected(sub) => Self::Sub(sub),
            }
        }
    }

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
                .unwrap_or_else(|_| "6379".to_string())
                .parse()
                .ok();

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

    #[derive(Clone)]
    pub struct Redis {
        con: redis::aio::ConnectionManager,
    }

    impl Redis {
        pub async fn set_keep_ttl<V>(&self, key: &Key<'_>, value: V) -> Result<()>
        where
            V: redis::ToSingleRedisArg + Send + Sync,
        {
            trace!("SET with KEEPTTL -> {key:?}");
            let mut con = self.con.clone();
            con.set_options::<_, _, ()>(
                &key,
                value,
                SetOptions::default().with_expiration(redis::SetExpiry::KEEPTTL),
            )
            .await?;
            Ok(())
        }

        pub async fn set_ex<V>(&self, key: &Key<'_>, value: V, ttl: Duration) -> Result<()>
        where
            V: redis::ToSingleRedisArg + Send + Sync,
        {
            trace!("SET_EX -> {key:?}");
            let mut con = self.con.clone();
            con.set_ex::<_, _, ()>(&key, value, ttl.as_secs()).await?;
            Ok(())
        }

        pub async fn get<V>(&self, key: &Key<'_>) -> Result<V>
        where
            V: redis::FromRedisValue,
        {
            let mut con = self.con.clone();
            match con.get::<_, Option<V>>(&key).await {
                Ok(value) => {
                    let status = if value.is_some() { "Hit" } else { "Miss" };
                    trace!("GET ({status}) -> {key:?}");

                    value.ok_or(Error::NotFound(key.to_string()))
                }
                Err(e) => {
                    error!("Failed to GET on {key:?}. Reason: {e:?}");
                    Err(Error::from(e))
                }
            }
        }

        pub async fn ttl(&self, key: &Key<'_>) -> Result<Duration> {
            let mut con = self.con.clone();
            trace!("TTL -> {key:?}");
            match con.ttl::<_, i64>(key).await {
                Ok(ttl) if ttl > 0 => Ok(Duration::from_secs(ttl.cast_unsigned())),
                Ok(v) => {
                    if v == -1 {
                        warn!("Key {key:?} has no expiration");
                        Err(Error::NoExpiration(key.to_string()))
                    } else if v == -2 {
                        warn!("Key {key:?} does not exist");
                        Err(Error::KeyDoesNotExist(key.to_string()))
                    } else {
                        error!("Invalid TTL: {v} for key {key:?}");
                        Err(Error::Unexpected("Invalid TTL value".to_string()))
                    }
                }
                Err(e) => Err(Error::from(e)),
            }
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
}
