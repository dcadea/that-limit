use std::{net::IpAddr, sync::Arc};

use axum::{
    Extension,
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderName},
    middleware::Next,
    response::Response,
};

use crate::core::{bucket, store::Store};

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
        .map(bucket::Id::Protected);

    let public = request
        .extensions()
        .get::<ClientIp>()
        .map(|ClientIp(ip)| *ip)
        .map(bucket::Id::Public);

    match protected.or(public) {
        Some(id) => {
            request.extensions_mut().insert(id);
            Ok(next.run(request).await)
        }
        None => Err(super::Error::Unauthorized),
    }
}

pub async fn lease_tokens(
    Extension(b_id): Extension<bucket::Id>,
    store: State<Arc<Store>>,
    request: Request<Body>,
    next: Next,
) -> super::Result<Response> {
    if store.check(&b_id)? {
        return Ok(next.run(request).await);
    }

    store.lease(b_id).await?;

    Ok(next.run(request).await)
}
