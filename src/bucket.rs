use std::{
    fmt::Display,
    net::Ipv4Addr,
    ops::Add,
    time::{Duration, SystemTime},
};

use serde::{Deserialize, Serialize};
use serde_with::{DurationSeconds, serde_as};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum Id {
    Public(Ipv4Addr),
    Protected(String),
}

impl Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Public(ip) => write!(f, "public:{ip}"),
            Self::Protected(sub) => write!(f, "protected:{sub}"),
        }
    }
}

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Criteria {
    Sub,
    Ip,
}

#[serde_as]
#[derive(Deserialize, Serialize, Debug)]
pub struct Config {
    criteria: Criteria,
    pub quota: u128,
    #[serde_as(as = "DurationSeconds<u64>")]
    pub reset_in: Duration,
}

#[derive(Debug)]
pub struct Bucket {
    pub tokens: u128,
    pub expires_at: SystemTime,
}

impl Bucket {
    pub fn new(tokens: u128, ttl: Duration) -> Self {
        let expires_at = SystemTime::now().add(ttl);

        Self { tokens, expires_at }
    }
}
