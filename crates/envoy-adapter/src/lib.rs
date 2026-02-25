use log::error;
use that_limit_core::StoreError;

mod app;
mod store;

pub use app::start_envoy;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Unauthorized")]
    Unauthorized,
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<Error> for tonic::Status {
    fn from(e: Error) -> Self {
        error!("Mapping error to envoy gRPC response: {e:?}");

        match e {
            Error::Unauthorized => Self::unauthenticated("unauthenticated"),
            Error::Store(e) => match e {
                StoreError::Exhausted(id, _) => {
                    Self::resource_exhausted(format!("Identity: {id} consumed all tokens"))
                }
                StoreError::Cache(_) => Self::internal("Internal Server Error"),
            },
        }
    }
}
