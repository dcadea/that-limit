use log::error;

use crate::core;

mod bootstrap;
mod store;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Unauthorized")]
    Unauthorized,
}

impl From<Error> for tonic::Status {
    fn from(e: Error) -> Self {
        error!("Mapping error to gRPC response: {e:?}");

        match e {
            Error::Unauthorized => Self::unauthenticated("unauthenticated"),
        }
    }
}

impl From<core::store::Error> for tonic::Status {
    fn from(e: core::store::Error) -> Self {
        error!("Mapping error to gRPC response: {e:?}");

        match e {
            core::store::Error::Exhausted(id, _) => {
                Self::resource_exhausted(format!("Identity: {id} consumed all tokens"))
            }
            core::store::Error::Cache(_) => Self::internal("Internal Server Error"),
        }
    }
}
