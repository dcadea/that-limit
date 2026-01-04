use serde::Deserialize;
use serde::Serialize;

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
    use axum::{Json, http::StatusCode};
    use tokio::fs;

    use crate::cfg::Config;

    pub async fn get() -> Result<Json<Config>, (StatusCode, String)> {
        match fs::read_to_string("static/config.json").await {
            Ok(contents) => match serde_json::from_str::<Config>(&contents) {
                Ok(c) => Ok(Json(c)),
                Err(e) => Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Invalid config format: {}", e),
                )),
            },
            Err(e) => Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Could not read config: {}", e),
            )),
        }
    }
}
