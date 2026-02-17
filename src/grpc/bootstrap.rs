use std::env;

use envoy_types::pb::envoy::service::ratelimit::v3::rate_limit_service_server::RateLimitServiceServer;
use tonic::transport::Server;

use crate::{
    bootstrap::{App, shutdown_signal},
    grpc::store,
};

impl App {
    pub async fn run_grpc(&self) {
        let port = env::var("GRPC_PORT").unwrap_or_else(|_| "50051".into());
        let addr = format!("0.0.0.0:{port}")
            .parse()
            .expect("Failed to parse server address");

        if let Err(e) = Server::builder()
            .add_service(RateLimitServiceServer::new(store::Service::new(
                self.store().clone(),
            )))
            .serve_with_shutdown(addr, shutdown_signal(self.shutdown_tx.clone()))
            .await
        {
            panic!("Failed to start application: {e:?}")
        }
    }
}
