use std::fmt::Debug;

use redis::RedisError;

mod action;
mod config;
mod model;
pub use action::{Action, Incr, Lease};
pub use config::Config as CacheConfig;
pub use model::Key;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Redis error occurred: {0}")]
    Redis(#[from] RedisError),
    #[error("Key {0} does not exist")]
    KeyDoesNotExist(String),
    #[error("Key {0} does not have expiration")]
    NoExpiration(String),
    #[error("Unexpected redis error: {0}")]
    Unexpected(String),
    #[error("Key {0} not found")]
    NotFound(String),
}

#[derive(Clone)]
pub struct Redis {
    con: redis::aio::ConnectionManager,
}

impl Redis {
    /// # Errors
    ///
    /// Will return `Err` if respective redis command fails to execute.
    pub async fn execute<A>(&self, action: A) -> Result<A::Output>
    where
        A: action::Action,
    {
        action.execute(&mut self.con.clone()).await
    }
}
