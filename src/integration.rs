pub mod cache {
    use std::{
        env,
        fmt::{Display, Formatter},
        net::Ipv4Addr,
    };

    use log::{error, trace, warn};
    use redis::{AsyncCommands, SetOptions};

    use crate::bucket;

    #[derive(Clone, Debug)]
    pub enum Key<'a> {
        Sub(&'a str),
        Ip(Ipv4Addr),
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
        pub async fn set_keep_ttl<V>(&self, key: &Key<'_>, value: V)
        where
            V: redis::ToSingleRedisArg + Send + Sync,
        {
            trace!("SET with KEEPTTL -> {key:?}");
            let mut con = self.con.clone();
            if let Err(e) = con
                .set_options::<_, _, ()>(
                    &key,
                    value,
                    SetOptions::default().with_expiration(redis::SetExpiry::KEEPTTL),
                )
                .await
            {
                error!("Failed to SET with KEEPTTL on {key:?}. Reason: {e:?}");
            }
        }

        pub async fn set_ex<V>(&self, key: &Key<'_>, value: V, ttl: u64)
        where
            V: redis::ToSingleRedisArg + Send + Sync,
        {
            trace!("SET_EX -> {key:?}");
            let mut con = self.con.clone();
            if let Err(e) = con.set_ex::<_, _, ()>(&key, value, ttl).await {
                error!("Failed to SET_EX on {key:?}. Reason: {e:?}");
            }
        }

        pub async fn get<V>(&self, key: &Key<'_>) -> Option<V>
        where
            V: redis::FromRedisValue,
        {
            let mut con = self.con.clone();
            match con.get::<_, Option<V>>(&key).await {
                Ok(value) => {
                    let status = if value.is_some() { "Hit" } else { "Miss" };
                    trace!("GET ({status}) -> {key:?}");
                    value
                }
                Err(e) => {
                    error!("Failed to GET on {key:?}. Reason: {e:?}");
                    None
                }
            }
        }

        pub async fn ttl(&self, key: &Key<'_>) -> Option<u64> {
            let mut con = self.con.clone();
            match con.ttl::<_, i64>(key).await {
                Ok(ttl) if ttl > 0 => Some(ttl as u64),
                Ok(_) => {
                    // -1 - key exists but no expiration
                    // -2 - key does not exist
                    None
                }
                Err(e) => {
                    error!("Failed to identify TTL on {key:?}. Reason: {e:?}");
                    None
                }
            }
        }

        pub async fn del(&self, key: &Key<'_>) {
            let mut con = self.con.clone();
            if let Err(e) = con.del::<_, ()>(&key).await {
                error!("Failed to DEL on {key:?}. Reason: {e:?}");
            }
        }
    }
}
