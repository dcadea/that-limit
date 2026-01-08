pub mod cache {
    use std::{
        env,
        fmt::{Display, Formatter},
        net::Ipv4Addr,
    };

    use log::{error, trace, warn};
    use redis::AsyncCommands;

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
        pub async fn set<V>(&self, key: Key<'_>, value: V)
        where
            V: redis::ToSingleRedisArg + Send + Sync,
        {
            trace!("SET -> {key:?}");
            let mut con = self.con.clone();
            if let Err(e) = con.set::<_, _, ()>(&key, value).await {
                error!("Failed to SET on {key:?}. Reason: {e:?}");
            }
        }

        pub async fn get<V>(&self, key: Key<'_>) -> Option<V>
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

        pub async fn del(&self, key: Key<'_>) {
            let mut con = self.con.clone();
            if let Err(e) = con.del::<_, ()>(&key).await {
                error!("Failed to DEL on {key:?}. Reason: {e:?}");
            }
        }
    }
}
