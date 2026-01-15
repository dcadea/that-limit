use std::fs;
use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;
use serde_with::{DurationSeconds, serde_as};

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}

#[serde_as]
#[derive(Clone, PartialEq, Eq, Deserialize, Serialize, Debug)]
pub struct Quota {
    quota: u64,
    #[serde_as(as = "DurationSeconds<u64>")]
    reset_in: Duration,
}

#[serde_as]
#[derive(Clone, PartialEq, Eq, Deserialize, Serialize, Debug)]
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

#[derive(Clone, PartialEq, Eq, Deserialize, Debug, Serialize)]
pub struct Config {
    pub sync_every: u8,
    pub protected: Criteria,
    pub public: Criteria,
}

pub mod handler {

    use std::sync::Arc;

    use axum::{Json, extract::State};

    use crate::cfg::Config;

    pub async fn get(config: State<Arc<Config>>) -> Json<Config> {
        Json(Config::clone(&config))
    }
}

pub fn get(path: &str) -> Result<Config, Error> {
    let content = fs::read_to_string(path)?;

    let config = serde_json::from_str::<Config>(&content)?;

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
                sync_every: 50,
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
