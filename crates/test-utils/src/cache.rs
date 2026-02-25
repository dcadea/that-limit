use std::{fmt::Debug, marker::PhantomData};

use log::{error, trace};
use redis::AsyncCommands;
use that_limit_cache::{Action, Error, Key, Result};

pub struct Get<K, R> {
    key: K,
    _r: PhantomData<R>,
}

impl<K, R> Get<K, R> {
    pub const fn new(key: K) -> Self {
        Self {
            key,
            _r: PhantomData,
        }
    }
}

impl<K, R> Action for Get<K, R>
where
    K: Key + Sync + Debug + ToString,
    R: redis::FromRedisValue,
{
    type Output = R;

    async fn execute(self, con: &mut redis::aio::ConnectionManager) -> Result<Self::Output> {
        let key = self.key.to_key();
        match con.get::<_, Option<Self::Output>>(&key).await {
            Ok(value) => {
                let status = if value.is_some() { "Hit" } else { "Miss" };
                trace!("GET ({status}) -> {key:?}");

                value.ok_or(Error::NotFound(key.clone()))
            }
            Err(e) => {
                error!("Failed to GET on {key:?}. Reason: {e:?}");
                Err(Error::from(e))
            }
        }
    }
}
