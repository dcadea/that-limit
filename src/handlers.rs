use axum::{Json, http::StatusCode};
use tokio::fs;

use crate::cfg::Config;

pub async fn root() -> &'static str {
    "Hello World"
}

pub async fn config_route() -> Result<Json<Config>, (StatusCode, String)> {
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
