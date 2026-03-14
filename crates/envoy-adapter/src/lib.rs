use std::num::TryFromIntError;

use log::error;

mod app;
mod store;

pub use app::start_envoy;
use that_limit_core::StoreError;

pub type EnvoyResult<T> = std::result::Result<T, Error>;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Unauthorized")]
    Unauthorized,
    #[error("IP Malformed")]
    IpMalformed,
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    ParseInt(#[from] TryFromIntError),
}

impl From<Error> for tonic::Status {
    fn from(e: Error) -> Self {
        error!("Mapping error to envoy gRPC response: {e:?}");

        match e {
            Error::Unauthorized => Self::unauthenticated("Unauthenticated"),
            Error::IpMalformed => Self::invalid_argument("IP Malformed"),
            Error::Store(e) => Self::internal(e.to_string()),
            Error::ParseInt(e) => Self::internal(e.to_string()),
        }
    }
}
