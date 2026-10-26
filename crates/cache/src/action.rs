use std::{fmt::Display, marker::PhantomData};

use log::trace;

use crate::model::Key;

/// Output of a [`Script`]: integers, strings, `Option`s, `Vec`s and tuples of these.
pub trait FromScript: redis::FromRedisValue {}

impl<T: redis::FromRedisValue> FromScript for T {}

/// Lua script run atomically on Redis with `EVALSHA` (falls back to `EVAL`).
pub struct Script<T> {
    src: &'static str,
    keys: Vec<String>,
    args: Vec<String>,
    _out: PhantomData<T>,
}

impl<T: FromScript> Script<T> {
    #[must_use]
    pub const fn new(src: &'static str) -> Self {
        Self {
            src,
            keys: Vec::new(),
            args: Vec::new(),
            _out: PhantomData,
        }
    }

    #[must_use]
    pub fn key(mut self, key: &impl Key) -> Self {
        self.keys.push(key.to_key());
        self
    }

    #[must_use]
    pub fn arg(mut self, arg: impl Display) -> Self {
        self.args.push(arg.to_string());
        self
    }

    pub(crate) async fn execute(self, con: &mut redis::aio::ConnectionManager) -> super::Result<T> {
        trace!("EVAL -> {:?}", self.keys);

        let script = redis::Script::new(self.src);
        let mut invocation = script.prepare_invoke();
        for key in &self.keys {
            invocation.key(key);
        }
        for arg in &self.args {
            invocation.arg(arg);
        }

        Ok(invocation.invoke_async(con).await?)
    }
}
