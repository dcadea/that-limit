use axum::{Json, http::StatusCode, response::IntoResponse};
use log::error;
use serde::Serialize;

use crate::core;

mod bootstrap;
mod middleware;
mod state;
mod store;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Cfg(#[from] core::cfg::Error),
    #[error(transparent)]
    Store(#[from] core::store::Error),
    #[error("Unauthorized")]
    Unauthorized,
}

#[derive(Serialize)]
struct ErrorResponse {
    error_message: String,
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        error!("Mapping error to HTTP response: {self:?}");

        let (status, error_message) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized".to_string()),
            Self::Store(e) => match e {
                core::store::Error::Exhausted(id) => (
                    StatusCode::TOO_MANY_REQUESTS,
                    format!("Identity: {id} consumed all tokens"),
                ),
                core::store::Error::Cache(_) => (
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
