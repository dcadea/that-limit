use testcontainers_modules::testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner};
use that_limit_cache::{Key, Script};

#[expect(clippy::missing_panics_doc, reason = "infallible")]
pub async fn init_redis() -> ContainerAsync<testcontainers_modules::redis::Redis> {
    testcontainers_modules::redis::Redis::default()
        .with_tag("7")
        .start()
        .await
        .unwrap()
}

#[must_use]
pub fn get(key: &impl Key) -> Script<Option<u64>> {
    Script::new("return redis.call('GET', KEYS[1])").key(key)
}
