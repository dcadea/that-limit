use std::{string::ToString, sync::Arc};

use axum::{
    Extension,
    body::Body,
    extract::{Request, State},
    http::HeaderMap,
    middleware::Next,
    response::Response,
};
use axum_client_ip::ClientIp;

use crate::{bucket, error, store::Store};

pub async fn extract_identifier(
    headers: HeaderMap,
    mut request: Request<Body>,
    next: Next,
) -> crate::Result<Response> {
    let protected = headers
        .get("user_id")
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
        None => Err(error::Error::Unauthorized),
    }
}

pub async fn lease_tokens(
    Extension(b_id): Extension<bucket::Id>,
    store: State<Arc<Store>>,
    request: Request<Body>,
    next: Next,
) -> crate::Result<Response> {
    if store.check(&b_id)? {
        return Ok(next.run(request).await);
    }

    store.lease(&b_id).await?;

    Ok(next.run(request).await)
}
