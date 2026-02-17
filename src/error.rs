use std::env::VarError;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    VarError(#[from] VarError),
}
