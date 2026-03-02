use std::{env, sync::Arc};

use envoy_types::pb::envoy::service::ratelimit::v3::rate_limit_service_server::RateLimitServiceServer;
use log::info;
use that_limit_core::Store;
use tonic::transport::Server;

use crate::store;

/// # Panics
///
/// Will panic if could not bind to specified port or port is malformed.
pub async fn start_envoy<F>(store: Arc<Store>, signal: F)
where
    F: Future<Output = ()>,
{
    let port = env::var("ENVOY_PORT").unwrap_or_else(|_| "50051".into());
    let addr = format!("0.0.0.0:{port}")
        .parse()
        .expect("Failed to parse server address");

    match Server::builder()
        .add_service(RateLimitServiceServer::new(store::Service::new(
            store.clone(),
        )))
        .serve_with_shutdown(addr, signal)
        .await
    {
        Ok(_) => info!("Started on port: {port}"),
        Err(e) => panic!("Failed to start application: {e:?}"),
    }
}
