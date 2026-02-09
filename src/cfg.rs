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

#[derive(Clone, Deserialize, Debug, Serialize)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub struct Config {
    pub protected: Criteria,
    pub public: Criteria,
}

#[cfg(test)]
impl Config {
    pub fn test() -> Self {
        Self {
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

    pub fn with_quota(quota: u64) -> Self {
        Self {
            protected: Criteria::Sub(Quota {
                quota,
                reset_in: Duration::from_secs(600),
            }),
            public: Criteria::Ip(Quota {
                quota,
                reset_in: Duration::from_hours(1),
            }),
        }
    }
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

    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use crate::{init_router, state::test};

    use super::*;

    #[test]
    fn should_create_config_from_file() {
        let c = get("tests/fixtures/valid.json");

        assert_eq!(
            c.unwrap(),
            Config {
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

    #[tokio::test]
    async fn should_respond_ok_on_get_config() {
        let ts = test::State::new().await;
        let app = init_router(ts.app_state().clone());

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/config")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(StatusCode::OK, response.status());

        let expected = Config {
            protected: Criteria::Sub(Quota {
                quota: 10000,
                reset_in: Duration::from_secs(600),
            }),
            public: Criteria::Ip(Quota {
                quota: 500,
                reset_in: Duration::from_hours(1),
            }),
        };

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let actual: Config = serde_json::from_slice(&body).unwrap();

        assert_eq!(expected, actual);
    }
}
