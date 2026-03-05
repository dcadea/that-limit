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

    let store_service = store::Service::new(store.clone());

    info!("Starting on port: {port}");

    match Server::builder()
        .add_service(RateLimitServiceServer::new(store_service))
        .serve_with_shutdown(addr, signal)
        .await
    {
        Ok(()) => info!("Started on port: {port}"),
        Err(e) => panic!("Failed to start application: {e:?}"),
    }
}

#[cfg(test)]
pub mod test {
    use envoy_types::pb::envoy::service::ratelimit::v3::{
        rate_limit_service_client::RateLimitServiceClient,
        rate_limit_service_server::RateLimitServiceServer,
    };
    use hyper_util::rt::TokioIo;
    use tokio::io::DuplexStream;
    use tonic::transport::{Channel, Endpoint, Server, Uri};
    use tower::service_fn;

    use crate::store::Service;

    pub async fn init_app(
        svc: Service,
        (client, server): (DuplexStream, DuplexStream),
    ) -> RateLimitServiceClient<Channel> {
        tokio::spawn(async move {
            Server::builder()
                .add_service(RateLimitServiceServer::new(svc))
                .serve_with_incoming(tokio_stream::once(Ok::<_, std::io::Error>(server)))
                .await
                .unwrap();
        });

        let mut client = Some(client);
        RateLimitServiceClient::new(
            Endpoint::from_static("http://[::]:50051")
                .connect_with_connector(service_fn(move |_: Uri| {
                    let client = client.take();

                    async move {
                        if let Some(client) = client {
                            Ok(TokioIo::new(client))
                        } else {
                            Err(std::io::Error::other("Client already taken"))
                        }
                    }
                }))
                .await
                .unwrap(),
        )
    }
}
