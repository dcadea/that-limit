use std::sync::Arc;

use envoy_types::pb::envoy::service::ratelimit::v3::{
    RateLimitRequest, RateLimitResponse, rate_limit_service_server::RateLimitService,
};
use tonic::{Code, Request, Response, Status};

use crate::core::{bucket, store::Store};

#[derive(Clone)]
pub struct Service {
    store: Arc<Store>,
}

impl Service {
    pub const fn new(store: Arc<Store>) -> Self {
        Self { store }
    }
}

#[tonic::async_trait]
impl RateLimitService for Service {
    async fn should_rate_limit(
        &self,
        request: Request<RateLimitRequest>,
    ) -> Result<Response<RateLimitResponse>, Status> {
        let req = request.into_inner();
        let b_id = extract_identifier(&req)?;

        if !self.store.check(&b_id)? {
            self.store.lease(b_id.clone()).await?;
        }

        let tokens_left = self.store.consume(&b_id);

        let response = RateLimitResponse {
            overall_code: if tokens_left > 0 {
                Code::Ok as i32
            } else {
                Code::ResourceExhausted as i32
            },
            ..Default::default()
        };

        Ok(Response::new(response))
    }
}

fn extract_identifier(req: &RateLimitRequest) -> super::Result<bucket::Id> {
    req.descriptors
        .iter()
        .flat_map(|d| &d.entries)
        .find_map(|entry| match entry.key.as_str() {
            "user_id" => Some(bucket::Id::Protected(entry.value.clone())),
            "remote_address" => entry.value.parse().map(bucket::Id::Public).ok(),
            _ => None,
        })
        .ok_or(super::Error::Unauthorized)
}
