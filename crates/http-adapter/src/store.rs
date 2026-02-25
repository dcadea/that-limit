use std::sync::Arc;

use axum::{Extension, extract::State, http::HeaderName, response::IntoResponse};
use that_limit_core::{BucketId, Store};

// TODO: see https://datatracker.ietf.org/doc/draft-ietf-httpapi-ratelimit-headers/
const RATE_LIMIT_HEADER_NAME: HeaderName = HeaderName::from_static("ratelimit");

pub async fn consume(
    Extension(bucket_id): Extension<BucketId>,
    store: State<Arc<Store>>,
) -> super::Result<impl IntoResponse> {
    let tokens_left = store.consume(bucket_id).await?;

    Ok(([(RATE_LIMIT_HEADER_NAME, format!("r={tokens_left}"))], ()))
}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use axum::{
        body::Body,
        http::{self, HeaderValue, Request, StatusCode},
    };
    use that_limit_core::Config;
    use that_limit_test_utils::config::ConfigExt;
    use tokio::time;
    use tower::ServiceExt;

    use super::*;
    use crate::{
        app::{init_router, test::TestApp},
        middleware::{FORWARDED, USER_ID, X_FORWARDED_FOR, X_REAL_IP},
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
        let expected = format!("r={}", config.protected.lease_size - 1);

        let actual = response
            .headers()
            .get(RATE_LIMIT_HEADER_NAME)
            .map(HeaderValue::to_str)
            .map(Result::unwrap)
            .unwrap();

        assert_eq!(expected, actual);
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
            let expected = format!("r={}", config.public.lease_size - 1);

            let actual = response
                .headers()
                .get(RATE_LIMIT_HEADER_NAME)
                .map(HeaderValue::to_str)
                .map(Result::unwrap)
                .unwrap();

            assert_eq!(expected, actual);
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
        let expected = format!("r={}", config.protected.lease_size - 2);

        let actual = response
            .headers()
            .get(RATE_LIMIT_HEADER_NAME)
            .map(HeaderValue::to_str)
            .map(Result::unwrap)
            .unwrap();

        assert_eq!(expected, actual);
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
        let app = TestApp::with_config(config).await;
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
        let app = TestApp::with_config(config).await;
        let r = init_router(app.app_state().clone());

        // first request leases first batch of tokens
        let response = r.clone().oneshot(consume_request(TEST_USER)).await.unwrap();
        assert_eq!(StatusCode::OK, response.status());
        let actual = response
            .headers()
            .get(RATE_LIMIT_HEADER_NAME)
            .map(HeaderValue::to_str)
            .map(Result::unwrap)
            .unwrap();
        assert_eq!("r=1", actual);

        // this will consume last token
        let response = r.clone().oneshot(consume_request(TEST_USER)).await.unwrap();
        assert_eq!(StatusCode::OK, response.status());
        let actual = response
            .headers()
            .get(RATE_LIMIT_HEADER_NAME)
            .map(HeaderValue::to_str)
            .map(Result::unwrap)
            .unwrap();
        assert_eq!("r=0", actual);

        // one more request to lease new batch of tokens
        let response = r.oneshot(consume_request(TEST_USER)).await.unwrap();
        assert_eq!(StatusCode::OK, response.status());
        let actual = response
            .headers()
            .get(RATE_LIMIT_HEADER_NAME)
            .map(HeaderValue::to_str)
            .map(Result::unwrap)
            .unwrap();
        assert_eq!("r=1", actual);
    }

    #[tokio::test]
    async fn should_return_zero_when_bucket_is_expired() {
        let reset_in = Duration::from_secs(1);
        let config = Config::default().with_protected_reset_in(reset_in);
        let app = TestApp::with_config(config).await;
        let r = init_router(app.app_state().clone());

        let expected = format!("r={}", app.config().protected.lease_size - 1);

        // trigger initial lease with bucket lifetime of 1 second (min allowed by redis)
        let response = r.clone().oneshot(consume_request(TEST_USER)).await.unwrap();
        assert_eq!(StatusCode::OK, response.status());
        let actual = response
            .headers()
            .get(RATE_LIMIT_HEADER_NAME)
            .map(HeaderValue::to_str)
            .map(Result::unwrap)
            .unwrap();
        assert_eq!(expected, actual);

        // wait for bucket to expire
        time::sleep(reset_in).await;

        let response = r.clone().oneshot(consume_request(TEST_USER)).await.unwrap();
        assert_eq!(StatusCode::OK, response.status());
        let actual = response
            .headers()
            .get(RATE_LIMIT_HEADER_NAME)
            .map(HeaderValue::to_str)
            .map(Result::unwrap)
            .unwrap();
        assert_eq!("r=0", actual);
    }
}
