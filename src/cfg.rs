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
    pub reset_in: u64,
}

#[derive(Deserialize, Debug, Serialize)]
pub struct Config {
    pub sync_every: u8,
    pub protected: BucketCfg,
    pub public: BucketCfg,
}

pub mod handler {

    use axum::{Json, http::StatusCode};

    use crate::cfg::Config;

    pub async fn get() -> Result<Json<Config>, (StatusCode, String)> {
        match super::get("static/config.json") {
            Ok(config) => Ok(Json(config)),
            // Err(err) => Err((StatusCode::INTERNAL_SERVER_ERROR, err)),
            Err(_) => Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error".to_string(),
            )),
        }
    }
}

pub fn get(path: &str) -> Result<Config, Error> {
    // let content = fs::read_to_string(path).map_err(|err| Error::Io(err))?;
    let content = fs::read_to_string(path)?;

    let config =
        // serde_json::from_str::<Config>(&content).map_err(|err| Error::Json(err))?;
        serde_json::from_str::<Config>(&content)?;

    Ok(config)
}
