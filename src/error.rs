use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::Serialize;

use crate::{cfg, integration::cache};

#[derive(Debug)]
pub enum Error {
    Cfg(cfg::Error),
    Io(std::io::Error),
    Cache(cache::Error),
    Unauthorized,
    Internal(String),
}

impl From<cfg::Error> for Error {
    fn from(e: cfg::Error) -> Self {
        Self::Cfg(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<cache::Error> for Error {
    fn from(e: cache::Error) -> Self {
        Self::Cache(e)
    }
}

#[derive(Serialize)]
struct ErrorResponse {
    error_message: String,
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        let (status, error_message) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized".to_string()),
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error".to_string(),
            ),
        };

        (status, Json(ErrorResponse { error_message })).into_response()
    }
}
