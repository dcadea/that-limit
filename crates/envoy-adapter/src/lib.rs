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
    #[error("IP Malformed")]
    IpMalformed,
    #[error(transparent)]
    Store(#[from] StoreError),
}

// TODO: only cache error should map to status
impl From<Error> for tonic::Status {
    fn from(e: Error) -> Self {
        error!("Mapping error to envoy gRPC response: {e:?}");

        match e {
            Error::Unauthorized => Self::unauthenticated("Unauthenticated"),
            Error::IpMalformed => Self::invalid_argument("IP Malformed"),
            Error::Store(_) => Self::internal("Internal Server Error"),
        }
    }
}
