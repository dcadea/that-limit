use std::sync::Arc;

use axum::{Extension, Json, extract::State};
use serde::{Deserialize, Serialize};

use crate::core::{bucket, store::Store};

#[derive(Serialize, Deserialize)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub struct ConsumeResponse {
    tokens_left: u64,
}

pub async fn consume(
    Extension(bucket_id): Extension<bucket::Id>,
    store: State<Arc<Store>>,
) -> super::Result<Json<ConsumeResponse>> {
    let tokens_left = store.consume(bucket_id).await?;

    Ok(Json(ConsumeResponse { tokens_left }))
}

#[cfg(test)]
mod test {
    use super::*;

    use std::time::Duration;

    use axum::{
        body::Body,
        http::{self, Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use tokio::time;
    use tower::ServiceExt;

    use crate::{
        core::cfg::Config,
        http::bootstrap::{init_router, test::TestApp},
        http::middleware::{FORWARDED, USER_ID, X_FORWARDED_FOR, X_REAL_IP},
    };

    const TEST_USER: &str = "valera";

    fn consume_request(user_id: &str) -> Request<Body> {
        Request::builder()
            .method(http::Method::POST)
            .uri("/consume")
            .header(USER_ID, user_id)
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn should_respond_ok_on_consume_authorized() {
        let app = TestApp::new().await;
        let r = init_router(app.app_state().clone());

        let response = r.oneshot(consume_request(TEST_USER)).await.unwrap();

        assert_eq!(StatusCode::OK, response.status());

        let config = app.config();
        let expexted = ConsumeResponse {
            tokens_left: config.protected.lease_size() - 1,
        };

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let actual: ConsumeResponse = serde_json::from_slice(&body).unwrap();

        assert_eq!(expexted, actual);
    }

    #[tokio::test]
    async fn should_respond_ok_on_consume_public() {
        let app = TestApp::new().await;
        let r = init_router(app.app_state().clone());

        let ip_headers = [
            (FORWARDED, "89.28.75.89"),
            (X_FORWARDED_FOR, "28.75.89.89"),
            (X_REAL_IP, "89.75.28.89"),
            (FORWARDED, "2001:db8::1"),
            (X_FORWARDED_FOR, "fe80::1"),
            (X_REAL_IP, "::ffff:192.0.2.128"),
        ];

        for (h, ip) in ip_headers {
            let response = r
                .clone()
                .oneshot(
                    Request::builder()
                        .method(http::Method::POST)
                        .uri("/consume")
                        .header(h, ip)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(StatusCode::OK, response.status());

            let config = app.config();
            let expexted = ConsumeResponse {
                tokens_left: config.public.lease_size() - 1,
            };

            let body = response.into_body().collect().await.unwrap().to_bytes();
            let actual: ConsumeResponse = serde_json::from_slice(&body).unwrap();

            assert_eq!(expexted, actual);
        }
    }

    #[tokio::test]
    async fn should_fail_on_consume_public_malformed() {
        let app = TestApp::new().await;
        let r = init_router(app.app_state().clone());

        let ip_headers = [
            (FORWARDED, ""),
            (X_FORWARDED_FOR, "1.2.3"),
            (X_REAL_IP, "256.1.1.1"),
            (FORWARDED, "1.2.3.4 "),
            (X_FORWARDED_FOR, "01.02.03.04"),
            (X_REAL_IP, "1..3.4"),
            (FORWARDED, ":"),
            (X_FORWARDED_FOR, ":::"),
            (X_REAL_IP, "2001::db8::1"),
            (FORWARDED, "2001:dg8::1"),
            (X_FORWARDED_FOR, "fe80::1%eth0"),
            (X_REAL_IP, "::ffff:999.1.1.1"),
            (FORWARDED, "12345::1"),
        ];

        for (h, ip) in ip_headers {
            let response = r
                .clone()
                .oneshot(
                    Request::builder()
                        .method(http::Method::POST)
                        .uri("/consume")
                        .header(h, ip)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(StatusCode::UNAUTHORIZED, response.status());
        }
    }

    #[tokio::test]
    async fn should_respond_ok_on_subsequent_consume() {
        let app = TestApp::new().await;
        let r = init_router(app.app_state().clone());

        // first call will lease
        let _ = r.clone().oneshot(consume_request(TEST_USER)).await.unwrap();

        // second call will skip lease and deduct from local store
        let response = r.oneshot(consume_request(TEST_USER)).await.unwrap();

        assert_eq!(StatusCode::OK, response.status());

        let config = app.config();
        let expexted = ConsumeResponse {
            tokens_left: config.protected.lease_size() - 2,
        };

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let actual: ConsumeResponse = serde_json::from_slice(&body).unwrap();

        assert_eq!(expexted, actual);
    }

    #[tokio::test]
    async fn should_fail_on_consume_unauthorized() {
        let app = TestApp::new().await;
        let r = init_router(app.app_state().clone());

        let response = r
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/consume")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(StatusCode::UNAUTHORIZED, response.status());
    }

    #[tokio::test]
    async fn should_return_exhausted_when_no_quota_left() {
        let config = Config::default()
            .with_protected_quota(1)
            .with_protected_lease_size(1);
        let app = TestApp::with_cfg(config).await;
        let r = init_router(app.app_state().clone());

        let _ = r.clone().oneshot(consume_request(TEST_USER)).await.unwrap();

        let response = r.oneshot(consume_request(TEST_USER)).await.unwrap();

        assert_eq!(StatusCode::TOO_MANY_REQUESTS, response.status());
    }

    #[tokio::test]
    async fn should_lease_more_tokens_when_bucket_is_exhausted_and_quota_not_exceeded() {
        let config = Config::default()
            .with_protected_quota(5)
            .with_protected_lease_size(2);
        let app = TestApp::with_cfg(config).await;
        let r = init_router(app.app_state().clone());

        // first request leases first batch of tokens
        let response = r.clone().oneshot(consume_request(TEST_USER)).await.unwrap();
        assert_eq!(StatusCode::OK, response.status());
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let actual: ConsumeResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(1, actual.tokens_left);

        // this will consume last token
        let response = r.clone().oneshot(consume_request(TEST_USER)).await.unwrap();
        assert_eq!(StatusCode::OK, response.status());
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let actual: ConsumeResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(0, actual.tokens_left);

        // one more request to lease new batch of tokens
        let response = r.oneshot(consume_request(TEST_USER)).await.unwrap();
        assert_eq!(StatusCode::OK, response.status());
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let actual: ConsumeResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(1, actual.tokens_left);
    }

    #[tokio::test]
    async fn should_return_zero_when_bucket_is_expired() {
        let reset_in = Duration::from_secs(1);
        let config = Config::default().with_protected_reset_in(reset_in);
        let app = TestApp::with_cfg(config).await;
        let r = init_router(app.app_state().clone());

        // trigger initial lease with bucket lifetime of 1 second (min allowed by redis)
        let response = r.clone().oneshot(consume_request(TEST_USER)).await.unwrap();
        assert_eq!(StatusCode::OK, response.status());
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let actual: ConsumeResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(app.config().protected.lease_size() - 1, actual.tokens_left);

        // wait for bucket to expire
        time::sleep(reset_in).await;

        let response = r.clone().oneshot(consume_request(TEST_USER)).await.unwrap();
        assert_eq!(StatusCode::OK, response.status());
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let actual: ConsumeResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(0, actual.tokens_left);
    }
}
