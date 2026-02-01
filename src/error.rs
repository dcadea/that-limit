use axum::{Json, http::StatusCode, response::IntoResponse};
use log::debug;
use serde::Serialize;

use crate::{cfg, store};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Cfg(#[from] cfg::Error),
    #[error(transparent)]
    Store(#[from] store::Error),
    #[error("Unauthorized")]
    Unauthorized,
}

#[derive(Serialize)]
struct ErrorResponse {
    error_message: String,
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        debug!("Mapping error to HTTP response: {self:?}");

        let (status, error_message) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized".to_string()),
            Self::Store(e) => match e {
                store::Error::Exhausted(id) => (
                    StatusCode::TOO_MANY_REQUESTS,
                    format!("Identity: {id} consumed all tokens"),
                ),
                store::Error::Locked(id) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Identity: {id} is locked"),
                ),
                store::Error::Cache(_) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal Server Error".to_string(),
                ),
            },
            Self::Cfg(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error".to_string(),
            ),
        };

        (status, Json(ErrorResponse { error_message })).into_response()
    }
}
