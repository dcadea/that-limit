use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::Serialize;

use crate::{cfg, integration::cache, store};

#[derive(Debug)]
pub enum Error {
    Cfg(cfg::Error),
    Cache(cache::Error),
    Store(store::Error),
    Unauthorized,
    Internal(String),
}

impl From<cfg::Error> for Error {
    fn from(e: cfg::Error) -> Self {
        Self::Cfg(e)
    }
}

impl From<cache::Error> for Error {
    fn from(e: cache::Error) -> Self {
        Self::Cache(e)
    }
}

impl From<store::Error> for Error {
    fn from(e: store::Error) -> Self {
        Self::Store(e)
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
            Self::Store(e) => match e {
                store::Error::Exhausted(id) => (
                    StatusCode::TOO_MANY_REQUESTS,
                    format!("Identity: {id} consumed all tokens"),
                ),
                store::Error::NotFound(id) => {
                    (StatusCode::NOT_FOUND, format!("Identity: {id} not found"))
                }
            },
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error".to_string(),
            ),
        };

        (status, Json(ErrorResponse { error_message })).into_response()
    }
}
