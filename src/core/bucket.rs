use std::{
    fmt::Display,
    net::IpAddr,
    ops::Add,
    time::{Duration, SystemTime},
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Id {
    Public(IpAddr),
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

#[derive(Debug)]
pub(super) struct Bucket {
    pub tokens: u64,
    pub expires_at: SystemTime,
    pub exhausted: bool,
}

impl Bucket {
    pub fn new(tokens: u64, ttl: Duration) -> Self {
        let expires_at = SystemTime::now().add(ttl);

        Self {
            tokens,
            expires_at,
            exhausted: false,
        }
    }
}
