use std::fs;
use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;
use serde_with::{DurationSeconds, serde_as};

type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Clone, Deserialize, Serialize, Debug)]
#[cfg_attr(test, derive(PartialEq, Eq))]
#[serde(rename_all = "snake_case")]
enum Criteria {
    Sub,
    Ip,
}

#[serde_as]
#[derive(Clone, Deserialize, Serialize, Debug)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub struct Policy {
    criteria: Criteria,
    quota: u64,
    lease_size: u64,
    #[serde_as(as = "DurationSeconds<u64>")]
    reset_in: Duration,
}

impl Policy {
    pub const fn quota(&self) -> u64 {
        self.quota
    }

    pub const fn lease_size(&self) -> u64 {
        self.lease_size
    }

    pub const fn reset_in(&self) -> Duration {
        self.reset_in
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
    pub cleanup: Cleanup,
    pub protected: Policy,
    pub public: Policy,
}

pub fn get(path: &str) -> Result<Config> {
    let content = fs::read(path)?;
    let config = serde_json::from_slice(&content)?;
    Ok(config)
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cleanup: Cleanup {
                enabled: false,
                interval: Duration::default(),
            },
            protected: Policy {
                criteria: Criteria::Sub,
                quota: 10000,
                lease_size: 2000,
                reset_in: Duration::from_secs(600),
            },
            public: Policy {
                criteria: Criteria::Ip,
                quota: 500,
                lease_size: 100,
                reset_in: Duration::from_hours(1),
            },
        }
    }
}

#[cfg(test)]
use crate::core::bucket;

#[cfg(test)]
impl Config {
    pub fn with_protected_quota(&self, quota: u64) -> Self {
        Self {
            protected: Policy {
                quota,
                ..self.protected.clone()
            },
            ..self.clone()
        }
    }

    pub fn with_protected_reset_in(&self, reset_in: Duration) -> Self {
        Self {
            protected: Policy {
                reset_in,
                ..self.protected.clone()
            },
            ..self.clone()
        }
    }

    pub fn with_protected_lease_size(&self, lease_size: u64) -> Self {
        Self {
            protected: Policy {
                lease_size,
                ..self.protected.clone()
            },
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
                cleanup: Cleanup {
                    enabled: true,
                    interval: Duration::from_secs(5),
                },
                protected: Policy {
                    criteria: Criteria::Sub,
                    quota: 10000,
                    lease_size: 2000,
                    reset_in: Duration::from_secs(600)
                },
                public: Policy {
                    criteria: Criteria::Ip,
                    quota: 500,
                    lease_size: 100,
                    reset_in: Duration::from_hours(1)
                }
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
