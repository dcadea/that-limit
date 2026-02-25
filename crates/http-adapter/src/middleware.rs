use std::net::IpAddr;

use axum::{
    body::Body,
    extract::Request,
    http::{HeaderMap, HeaderName},
    middleware::Next,
    response::Response,
};
use that_limit_core::BucketId;

#[derive(Clone)]
struct ClientIp(IpAddr);

pub const FORWARDED: HeaderName = HeaderName::from_static("forwarded");
pub const X_FORWARDED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");
pub const X_REAL_IP: HeaderName = HeaderName::from_static("x-real-ip");
pub const USER_ID: HeaderName = HeaderName::from_static("user_id");

pub async fn extract_ip(
    headers: HeaderMap,
    mut request: Request<Body>,
    next: Next,
) -> super::Result<Response> {
    if headers.get(USER_ID).is_some() {
        return Ok(next.run(request).await);
    }

    if let Some(ip) = headers
        .get(FORWARDED)
        .or_else(|| headers.get(X_FORWARDED_FOR))
        .or_else(|| headers.get(X_REAL_IP))
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<IpAddr>().ok())
        .map(ClientIp)
    {
        request.extensions_mut().insert(ip);
    }

    Ok(next.run(request).await)
}

pub async fn extract_identifier(
    headers: HeaderMap,
    mut request: Request<Body>,
    next: Next,
) -> super::Result<Response> {
    let protected = headers
        .get(USER_ID)
        .and_then(|id| id.to_str().ok())
        .map(ToString::to_string)
        .map(BucketId::Protected);

    let public = request
        .extensions()
        .get::<ClientIp>()
        .map(|ClientIp(ip)| *ip)
        .map(BucketId::Public);

    match protected.or(public) {
        Some(id) => {
            request.extensions_mut().insert(id);
            Ok(next.run(request).await)
        }
        None => Err(super::Error::Unauthorized),
    }
}
