use std::fs;

use serde::Deserialize;
use serde::Serialize;

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

// impl Display for Error {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         match self {
//             Error::Io(err) => write!(f, "IO Error: {}", err),
//             Error::Json(err) => write!(f, "JSON Error: {}", err),
//         }
//     }
// }

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Criteria {
    Sub,
    Ip,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct BucketCfg {
    criteria: Criteria,
    pub quota: u128,
    reset_in: u128,
}

#[derive(Deserialize, Debug, Serialize)]
pub struct Config {
    pub sync_every: u8,
    pub protected: BucketCfg,
    pub public: BucketCfg,
}

pub mod handler {
    use std::sync::Arc;

    use axum::{Json, extract::State, http::StatusCode};

    use crate::cfg::{Config, service::Service};

    pub async fn get(service: State<Arc<Service>>) -> Result<Json<Config>, (StatusCode, String)> {
        match service.get("static/config.json") {
            Ok(config) => Ok(Json(config)),
            // Err(err) => Err((StatusCode::INTERNAL_SERVER_ERROR, err)),
            Err(_) => Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error".to_string(),
            )),
        }
    }
}

pub mod service {
    use std::fs;

    use crate::cfg::{Config, Error};
    #[derive(Debug)]
    pub struct Service {}

    impl Service {
        pub fn new() -> Self {
            Self {}
        }

        pub fn get(&self, path: &str) -> Result<Config, Error> {
            // let content = fs::read_to_string(path).map_err(|err| Error::Io(err))?;
            let content = fs::read_to_string(path)?;

            let config =
                // serde_json::from_str::<Config>(&content).map_err(|err| Error::Json(err))?;
                serde_json::from_str::<Config>(&content) ?;

            Ok(config)
        }
    }
}

pub fn get(path: &str) -> Result<Config, Error> {
    // let content = fs::read_to_string(path).map_err(|err| Error::Io(err))?;
    let content = fs::read_to_string(path)?;

    let config =
        // serde_json::from_str::<Config>(&content).map_err(|err| Error::Json(err))?;
        serde_json::from_str::<Config>(&content) ?;

    Ok(config)
}
