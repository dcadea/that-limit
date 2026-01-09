use std::{
    fmt::Display,
    net::Ipv4Addr,
    ops::Add,
    time::{Duration, SystemTime, SystemTimeError, UNIX_EPOCH},
};

use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum Id {
    Public(Ipv4Addr),
    Protected(String),
}

impl Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Id::Public(ip) => write!(f, "public:{ip}"),
            Id::Protected(sub) => write!(f, "protected:{sub}"),
        }
    }
}

#[derive(Debug)]
pub struct Bucket {
    pub tokens: u128,
    pub expires_at: u128,
}

impl Bucket {
    pub fn new(tokens: u128, ttl: u64) -> Result<Self, SystemTimeError> {
        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.add(Duration::from_secs(ttl)))
            .map(|d| d.as_millis())?;

        Ok(Self { tokens, expires_at })
    }
}
