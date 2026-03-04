use std::time::Instant;

use axum::{
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::IntoResponse,
};
use log::error;
use that_limit_core::{ConfigError, StoreError};

mod app;
mod extractor;
mod state;
mod store;

pub use app::start_http;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Cfg(#[from] ConfigError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("Unauthorized")]
    Unauthorized,
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        error!("Mapping to HTTP response: {self:?}");

        let mut headers = HeaderMap::new();

        let status = match self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Store(e) => match e {
                StoreError::Exhausted(_, expires_at) => {
                    if let Some(duration) = expires_at.map(|ex| ex.duration_since(Instant::now())) {
                        headers.insert(header::RETRY_AFTER, HeaderValue::from(duration.as_secs()));
                    }
                    StatusCode::TOO_MANY_REQUESTS
                }
                StoreError::Cache(_) => StatusCode::INTERNAL_SERVER_ERROR,
            },
            Self::Cfg(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
        .into_response();

        (headers, status).into_response()
    }
}
