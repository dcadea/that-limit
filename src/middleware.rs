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

use crate::{
    bucket, error,
    integration::cache::{self, Redis},
    store::Store,
};

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

// TODO: make configurable based on number of nodes to not exceed bucket size
const LEASE_SIZE: u64 = 100;

pub async fn lease_tokens(
    Extension(b_id): Extension<bucket::Id>,
    store: State<Arc<Store>>,
    redis: State<Redis>,
    request: Request<Body>,
    next: Next,
) -> crate::Result<Response> {
    if store.check(&b_id)? {
        return Ok(next.run(request).await);
    }

    let key = cache::Key::from(&b_id);

    let tokens: cache::Result<u64> = redis.get(&key).await;

    let ttl = match tokens {
        Ok(tokens) => {
            let ttl = redis.ttl(&key).await?;

            let leased = if tokens >= LEASE_SIZE {
                tokens - LEASE_SIZE
            } else {
                tokens
            };
            redis.set_keep_ttl(&key, leased).await?;
            ttl
        }

        Err(cache::Error::NotFound(_)) => {
            let cfg = store.config();
            let ttl = cfg.protected.reset_in();

            redis
                .set_ex(&key, cfg.protected.quota() - LEASE_SIZE, ttl)
                .await?;

            ttl
        }

        Err(_) => {
            return Err(error::Error::Internal(
                "Failed to lookup tokens by key".to_string(),
            ));
        }
    };

    store.add(b_id, LEASE_SIZE, ttl);

    Ok(next.run(request).await)
}
