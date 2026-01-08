use std::{
    ops::Add,
    time::{Duration, SystemTime, SystemTimeError, UNIX_EPOCH},
};

use crate::cfg::BucketCfg;

#[derive(Debug)]
pub struct Bucket {
    pub tokens: u128,
    pub expires_at: u128,
}

impl Bucket {
    pub fn new(cfg: &BucketCfg) -> Result<Self, SystemTimeError> {
        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.add(Duration::from_secs(cfg.reset_in)))
            .map(|d| d.as_millis())?;

        Ok(Self {
            tokens: cfg.quota,
            expires_at,
        })
    }
}
