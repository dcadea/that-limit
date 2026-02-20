use std::{
    fmt::Display,
    net::IpAddr,
    ops::Add,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
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
    tokens: AtomicU64,
    expires_at: SystemTime,
    exhausted: AtomicBool,
}

impl Bucket {
    pub fn new(tokens: u64, ttl: Duration) -> Self {
        let expires_at = SystemTime::now().add(ttl);

        Self {
            tokens: AtomicU64::new(tokens),
            expires_at,
            exhausted: AtomicBool::new(false),
        }
    }

    pub fn tokens(&self) -> u64 {
        self.tokens.load(Ordering::Relaxed)
    }

    pub fn is_empty(&self) -> bool {
        self.tokens.load(Ordering::Relaxed) == 0
    }

    pub fn is_expired(&self) -> bool {
        self.expires_at <= SystemTime::now()
    }

    pub fn set_exhausted(&self) {
        self.exhausted.store(true, Ordering::Release);
    }

    pub fn is_exhausted(&self) -> bool {
        self.exhausted.load(Ordering::Acquire)
    }

    pub fn consume(&self) -> u64 {
        let mut current = self.tokens.load(Ordering::Relaxed);

        while current > 0 {
            if self
                .tokens
                .compare_exchange(current, current - 1, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                return current - 1;
            }
            current = self.tokens.load(Ordering::Relaxed);
        }

        0
    }
}
