use std::env;

use log::warn;

use crate::Redis;

#[derive(Clone)]
pub struct Config {
    host: String,
    port: u16,
}

impl Config {
    pub const fn new(host: String, port: u16) -> Self {
        Self { host, port }
    }
}

impl Default for Config {
    fn default() -> Self {
        warn!("Fallback to default REDIS config");
        Self::new(String::from("127.0.0.1"), 6379)
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

    /// # Panics
    ///
    /// Will panic if could not establish redis connection.
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
