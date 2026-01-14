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

#[derive(Clone, Deserialize, Debug, Serialize)]
pub struct Config {
    pub sync_every: u8,
    pub protected: bucket::Config,
    pub public: bucket::Config,
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
