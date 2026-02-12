use std::fs;
use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;
use serde_with::{DurationSeconds, serde_as};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[serde_as]
#[derive(Clone, Deserialize, Serialize, Debug)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub struct Quota {
    quota: u64,
    #[serde_as(as = "DurationSeconds<u64>")]
    reset_in: Duration,
}

#[serde_as]
#[derive(Clone, Deserialize, Serialize, Debug)]
#[cfg_attr(test, derive(PartialEq, Eq))]
#[serde(tag = "criteria", rename_all = "snake_case")]
pub enum Criteria {
    Sub(Quota),
    Ip(Quota),
}

impl Criteria {
    pub const fn quota(&self) -> u64 {
        match self {
            Self::Ip(q) | Self::Sub(q) => q,
        }
        .quota
    }

    pub const fn reset_in(&self) -> Duration {
        match self {
            Self::Ip(q) | Self::Sub(q) => q,
        }
        .reset_in
    }
}

#[serde_as]
#[derive(Clone, Deserialize, Debug, Serialize)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub struct Cleanup {
    pub enabled: bool,
    #[serde_as(as = "DurationSeconds<u64>")]
    pub interval: Duration,
}

#[derive(Clone, Deserialize, Debug, Serialize)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub struct Config {
    pub lease_size: u64,
    pub cleanup: Cleanup,
    pub protected: Criteria,
    pub public: Criteria,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            lease_size: 100,
            cleanup: Cleanup {
                enabled: false,
                interval: Duration::default(),
            },
            protected: Criteria::Sub(Quota {
                quota: 10000,
                reset_in: Duration::from_secs(600),
            }),
            public: Criteria::Ip(Quota {
                quota: 500,
                reset_in: Duration::from_hours(1),
            }),
        }
    }
}

#[cfg(test)]
use crate::bucket;

#[cfg(test)]
impl Config {
    pub fn with_protected_quota(&self, quota: u64) -> Self {
        Self {
            protected: Criteria::Sub(Quota {
                quota,
                reset_in: self.protected.reset_in(),
            }),
            ..self.clone()
        }
    }

    pub fn with_protected_reset_in(&self, reset_in: Duration) -> Self {
        Self {
            protected: Criteria::Sub(Quota {
                quota: self.protected.quota(),
                reset_in,
            }),
            ..self.clone()
        }
    }

    pub fn with_lease_size(&self, lease_size: u64) -> Self {
        Self {
            lease_size,
            ..self.clone()
        }
    }

    pub fn with_cleanup(&self, cleanup: Cleanup) -> Self {
        Self {
            cleanup,
            ..self.clone()
        }
    }

    pub const fn quota(&self, b_id: &bucket::Id) -> u64 {
        match b_id {
            bucket::Id::Public(_) => self.public.quota(),
            bucket::Id::Protected(_) => self.protected.quota(),
        }
    }

    pub const fn reset_in(&self, b_id: &bucket::Id) -> Duration {
        match b_id {
            bucket::Id::Public(_) => self.public.reset_in(),
            bucket::Id::Protected(_) => self.protected.reset_in(),
        }
    }
}

pub fn get(path: &str) -> Result<Config, Error> {
    let content = fs::read(path)?;
    let config = serde_json::from_slice(&content)?;
    Ok(config)
}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use super::*;

    #[test]
    fn should_create_config_from_file() {
        let c = get("tests/fixtures/valid.json");

        assert_eq!(
            c.unwrap(),
            Config {
                lease_size: 100,
                cleanup: Cleanup {
                    enabled: true,
                    interval: Duration::from_secs(5),
                },
                protected: Criteria::Sub(Quota {
                    quota: 10000,
                    reset_in: Duration::from_secs(600)
                }),
                public: Criteria::Ip(Quota {
                    quota: 500,
                    reset_in: Duration::from_hours(1)
                })
            }
        );
    }

    #[test]
    fn should_fail_when_file_does_not_exist() {
        let c = get("tests/fixtures/unknown.json");

        assert!(matches!(c.unwrap_err(), Error::Io(e) if e.kind() == std::io::ErrorKind::NotFound));
    }

    #[test]
    fn should_fail_when_config_has_invalid_format() {
        let c = get("tests/fixtures/invalid_type.json");

        assert!(
            matches!(c.unwrap_err(), Error::Json(e) if e.to_string().contains(r#"invalid type: string "600", expected u64"#))
        );
    }
}
