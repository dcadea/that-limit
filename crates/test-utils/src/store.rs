use std::sync::Arc;

use testcontainers_modules::testcontainers::ContainerAsync;
use that_limit_cache::CacheConfig;
use that_limit_core::{Command, Config, Store};
use tokio::sync::broadcast;

use crate::cache::init_redis;

#[expect(clippy::missing_panics_doc, reason = "infallible")]
pub async fn init_store(
    cfg: &Config,
) -> (
    Arc<Store>,
    ContainerAsync<testcontainers_modules::redis::Redis>,
) {
    let rc = init_redis().await;
    let host = rc.get_host().await.unwrap().to_string();
    let port = rc.get_host_port_ipv4(6379).await.unwrap();

    let shutdown_tx = if cfg.cleanup.enabled {
        let (tx, _) = broadcast::channel::<Command>(10);
        Some(tx)
    } else {
        None
    };

    let redis = CacheConfig::new(host, port).connect().await;
    let store = Store::new(cfg.clone(), redis, shutdown_tx);

    (store, rc)
}
