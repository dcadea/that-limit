use std::{net::IpAddr, sync::Arc};

use envoy_types::{
    ext_authz::v3::pb::HeaderValue,
    pb::envoy::service::ratelimit::v3::{
        RateLimitRequest, RateLimitResponse, rate_limit_service_server::RateLimitService,
    },
};
use that_limit_core::{BucketId, Store, StoreError};
use tonic::{Code, Request, Response, Status};

const X_RATELIMIT_REMAINING: &str = "x-ratelimit-remaining";

#[derive(Clone)]
pub struct Service {
    store: Arc<Store>,
}

impl Service {
    pub const fn new(store: Arc<Store>) -> Self {
        Self { store }
    }
}

impl Service {
    async fn consume(&self, b_id: BucketId) -> super::Result<(u64, Code)> {
        match self.store.consume(b_id).await {
            Ok(0) | Err(StoreError::Exhausted(_, _)) => Ok((0, Code::ResourceExhausted)),
            Ok(tokens_left) => Ok((tokens_left, Code::Ok)),
            Err(e) => Err(super::Error::from(e)),
        }
    }
}

#[tonic::async_trait]
impl RateLimitService for Service {
    async fn should_rate_limit(
        &self,
        request: Request<RateLimitRequest>,
    ) -> Result<Response<RateLimitResponse>, Status> {
        let req = request.into_inner();

        // TODO: introduce domain in config
        if req.domain != "that-limit" {
            return Ok(Response::new(RateLimitResponse {
                overall_code: Code::Ok as i32,
                ..Default::default()
            }));
        }

        let b_id = extract_identifier(&req)?;

        let (tokens_left, overall_code) = self.consume(b_id).await?;

        let response = RateLimitResponse {
            overall_code: overall_code as i32,
            response_headers_to_add: vec![HeaderValue {
                key: X_RATELIMIT_REMAINING.to_string(),
                value: tokens_left.to_string(),
                raw_value: Vec::new(),
            }],
            ..Default::default()
        };

        Ok(Response::new(response))
    }
}

fn extract_identifier(req: &RateLimitRequest) -> super::Result<BucketId> {
    let mut remote_ip_str = None;

    for d in &req.descriptors {
        for e in &d.entries {
            match e.key.as_str() {
                "user_id" => return Ok(BucketId::Protected(e.value.as_str().into())),
                "remote_address" => remote_ip_str = e.value.split(',').next(),
                _ => {}
            }
        }
    }

    if let Some(s) = remote_ip_str {
        return s.parse::<IpAddr>().map_or_else(
            |_| Err(super::Error::IpMalformed),
            |ip| Ok(BucketId::Public(ip)),
        );
    }

    Err(super::Error::Unauthorized)
}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use envoy_types::pb::envoy::{
        extensions::common::ratelimit::v3::{RateLimitDescriptor, rate_limit_descriptor::Entry},
        service::ratelimit::v3::RateLimitRequest,
    };
    use that_limit_core::Config;
    use that_limit_test_utils::{config::ConfigExt, store::init_store};
    use tokio::{io::duplex, time};
    use tonic::Code;

    use crate::{
        app::test::init_app,
        store::{Service, X_RATELIMIT_REMAINING},
    };

    const TEST_USER: &str = "valera";

    fn for_sub(s: &str) -> Entry {
        Entry {
            key: "user_id".to_string(),
            value: s.to_string(),
        }
    }

    fn for_ip(s: &str) -> Entry {
        Entry {
            key: "remote_address".to_string(),
            value: s.to_string(),
        }
    }

    fn request(identity_descriptor: Entry) -> RateLimitRequest {
        RateLimitRequest {
            domain: "that-limit".to_string(),
            descriptors: vec![RateLimitDescriptor {
                entries: vec![identity_descriptor],
                limit: None,
                hits_addend: None,
            }],
            hits_addend: 1,
        }
    }

    #[tokio::test]
    async fn should_respond_ok_on_consume_authorized() {
        let config = Config::default();
        let (store, _rc) = init_store(&config).await;
        let svc = Service::new(store);

        let mut client = init_app(svc, duplex(1024)).await;

        let req = request(for_sub(TEST_USER));

        let resp = client.should_rate_limit(tonic::Request::new(req)).await;

        assert!(resp.is_ok_and(|r| {
            let r = r.get_ref();
            r.overall_code == 0 // Code::Ok
                && r.response_headers_to_add
                    .iter()
                    .find(|h| h.key == X_RATELIMIT_REMAINING)
                    .is_some_and(|h| h.value == format!("{}", config.protected.lease_size - 1))
        }));
    }

    #[tokio::test]
    async fn should_respond_ok_on_consume_public() {
        let config = Config::default();
        let (store, _rc) = init_store(&config).await;
        let svc = Service::new(store);

        let mut client = init_app(svc, duplex(1024)).await;

        let ip_headers = [
            "89.28.75.89",
            "89.75.28.89, 75.89.28.89",
            "::ffff:192.0.2.128",
            "fe80::1, 2001:db8::1",
        ];

        for ip in ip_headers {
            let req = request(for_ip(ip));

            let resp = client.should_rate_limit(tonic::Request::new(req)).await;

            assert!(resp.is_ok_and(|r| {
                let r = r.get_ref();
                r.overall_code == 0
                    && r.response_headers_to_add
                        .iter()
                        .find(|h| h.key == X_RATELIMIT_REMAINING)
                        .is_some_and(|h| h.value == format!("{}", config.public.lease_size - 1))
            }));
        }
    }

    #[tokio::test]
    async fn should_fail_on_consume_public_malformed() {
        let config = Config::default();
        let (store, _rc) = init_store(&config).await;
        let svc = Service::new(store);

        let mut client = init_app(svc, duplex(1024)).await;

        let ip_headers = [
            "",
            "1.2.3",
            "256.1.1.1",
            "1.2.3.4 ",
            "01.02.03.04",
            "1..3.4",
            ":",
            ":::",
            "2001::db8::1",
            "2001:dg8::1",
            "fe80::1%eth0",
            "::ffff:999.1.1.1",
            "12345::1",
        ];

        for ip in ip_headers {
            let req = request(for_ip(ip));

            let resp = client.should_rate_limit(tonic::Request::new(req)).await;

            assert!(resp.is_err_and(|s| s.code() == Code::InvalidArgument));
        }
    }

    #[tokio::test]
    async fn should_respond_ok_on_subsequent_consume() {
        let config = Config::default();
        let (store, _rc) = init_store(&config).await;
        let svc = Service::new(store);

        let mut client = init_app(svc, duplex(1024)).await;

        // first call will lease
        let req = request(for_sub(TEST_USER));
        let _ = client.should_rate_limit(tonic::Request::new(req)).await;

        // second call will skip lease and deduct from local store
        let req = request(for_sub(TEST_USER));
        let resp = client.should_rate_limit(tonic::Request::new(req)).await;

        assert!(resp.is_ok_and(|r| {
            let r = r.get_ref();
            r.overall_code == 0
                && r.response_headers_to_add
                    .iter()
                    .find(|h| h.key == X_RATELIMIT_REMAINING)
                    .is_some_and(|h| h.value == format!("{}", config.protected.lease_size - 2))
        }));
    }

    #[tokio::test]
    async fn should_fail_on_consume_unauthorized() {
        let config = Config::default();
        let (store, _rc) = init_store(&config).await;
        let svc = Service::new(store);

        let mut client = init_app(svc, duplex(1024)).await;

        let req = RateLimitRequest {
            domain: "that-limit".to_string(),
            descriptors: vec![],
            hits_addend: 1,
        };

        let resp = client.should_rate_limit(tonic::Request::new(req)).await;

        assert!(resp.is_err_and(|s| s.code() == Code::Unauthenticated));
    }

    #[tokio::test]
    async fn should_return_exhausted_when_no_quota_left() {
        let config = Config::default()
            .with_protected_quota(1)
            .with_protected_lease_size(1);
        let (store, _rc) = init_store(&config).await;
        let svc = Service::new(store);

        let mut client = init_app(svc, duplex(1024)).await;

        let req = request(for_sub(TEST_USER));
        let _ = client.should_rate_limit(tonic::Request::new(req)).await;

        let req = request(for_sub(TEST_USER));
        let resp = client.should_rate_limit(tonic::Request::new(req)).await;

        assert!(resp.is_ok_and(|r| {
            let r = r.get_ref();
            r.overall_code == 8 // Code::ResourceExhausted
                && r.response_headers_to_add
                    .iter()
                    .find(|h| h.key == X_RATELIMIT_REMAINING)
                    .is_some_and(|h| h.value == "0")
        }));
    }

    #[tokio::test]
    async fn should_lease_more_tokens_when_bucket_is_exhausted_and_quota_not_exceeded() {
        let config = Config::default()
            .with_protected_quota(5)
            .with_protected_lease_size(2);
        let (store, _rc) = init_store(&config).await;
        let svc = Service::new(store);

        let mut client = init_app(svc, duplex(1024)).await;

        // first request leases first batch of tokens
        let req = request(for_sub(TEST_USER));
        let _ = client.should_rate_limit(tonic::Request::new(req)).await;

        // this will consume last token
        let req = request(for_sub(TEST_USER));
        let _ = client.should_rate_limit(tonic::Request::new(req)).await;

        // one more request to lease new batch of tokens
        let req = request(for_sub(TEST_USER));
        let resp = client.should_rate_limit(tonic::Request::new(req)).await;

        assert!(resp.is_ok_and(|r| {
            let r = r.get_ref();
            r.overall_code == 0
                && r.response_headers_to_add
                    .iter()
                    .find(|h| h.key == X_RATELIMIT_REMAINING)
                    .is_some_and(|h| h.value == "1")
        }));
    }

    #[tokio::test]
    async fn should_return_zero_when_bucket_is_expired() {
        let reset_in = Duration::from_secs(1);
        let config = Config::default().with_protected_reset_in(reset_in);
        let (store, _rc) = init_store(&config).await;
        let svc = Service::new(store);

        let mut client = init_app(svc, duplex(1024)).await;

        // trigger initial lease with bucket lifetime of 1 second (min allowed by redis)
        let req = request(for_sub(TEST_USER));
        let _ = client.should_rate_limit(tonic::Request::new(req)).await;

        time::sleep(reset_in).await;

        let req = request(for_sub(TEST_USER));
        let resp = client.should_rate_limit(tonic::Request::new(req)).await;

        assert!(resp.is_ok_and(|r| {
            let r = r.get_ref();
            r.overall_code == 8 // Code::ResourceExhausted
                && r.response_headers_to_add
                    .iter()
                    .find(|h| h.key == X_RATELIMIT_REMAINING)
                    .is_some_and(|h| h.value == "0")
        }));
    }
}
