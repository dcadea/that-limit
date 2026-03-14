use std::sync::Arc;

use axum::{extract::State, http::HeaderName, response::IntoResponse};
use that_limit_core::Store;

use crate::extractor::Identifier;

const RATE_LIMIT_HEADER_NAME: HeaderName = HeaderName::from_static("ratelimit");

pub async fn consume(
    Identifier(bucket_id): Identifier,
    store: State<Arc<Store>>,
) -> super::Result<impl IntoResponse> {
    let tokens_left = store.consume(bucket_id).await?;

    Ok(([(RATE_LIMIT_HEADER_NAME, format!("r={tokens_left}"))], ()))
}

#[cfg(test)]
mod test {
    use std::{ops::Sub, time::Duration};

    use axum::{
        body::Body,
        http::{self, HeaderValue, Request, StatusCode, header::RETRY_AFTER},
    };
    use that_limit_core::Config;
    use that_limit_test_utils::config::ConfigExt;
    use tower::ServiceExt;

    use super::*;
    use crate::{
        app::{init_router, test::TestApp},
        extractor::{USER_ID, X_FORWARDED_FOR, X_REAL_IP},
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
            (X_REAL_IP, "89.28.75.89"),
            (X_REAL_IP, "89.75.28.89"),
            (X_REAL_IP, "::ffff:192.0.2.128"),
            (X_FORWARDED_FOR, "28.75.89.89"),
            (X_FORWARDED_FOR, "fe80::1, 2001:db8::1"),
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
            (X_REAL_IP, ""),
            (X_FORWARDED_FOR, "1.2.3"),
            (X_REAL_IP, "256.1.1.1"),
            (X_FORWARDED_FOR, "1.2.3.4 "),
            (X_FORWARDED_FOR, "01.02.03.04"),
            (X_REAL_IP, "1..3.4"),
            (X_REAL_IP, ":"),
            (X_FORWARDED_FOR, ":::"),
            (X_REAL_IP, "2001::db8::1"),
            (X_FORWARDED_FOR, "2001:dg8::1"),
            (X_FORWARDED_FOR, "fe80::1%eth0"),
            (X_REAL_IP, "::ffff:999.1.1.1"),
            (X_REAL_IP, "12345::1"),
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

        let retry_after = app
            .config()
            .protected
            .reset_in
            .sub(Duration::from_secs(1))
            .as_secs()
            .to_string();
        let ra_header = response.headers().iter().find(|h| RETRY_AFTER.eq(h.0));
        assert!(
            ra_header.is_some_and(|h| HeaderValue::from_str(retry_after.as_str()).unwrap().eq(h.1)),
        );
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
}
