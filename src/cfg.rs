use std::fs;

use serde::Deserialize;
use serde::Serialize;

use crate::bucket;

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

#[derive(Deserialize, Debug, Serialize)]
pub struct Config {
    pub sync_every: u8,
    pub protected: bucket::Config,
    pub public: bucket::Config,
}

pub mod handler {

    use axum::Json;

    use crate::{cfg::Config, error};

    pub async fn get() -> Result<Json<Config>, error::Error> {
        let config = super::get("static/config.json")?;
        Ok(Json(config))
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
