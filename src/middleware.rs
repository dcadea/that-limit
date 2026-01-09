use std::sync::Arc;

use axum::{
    Extension,
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};

use crate::{
    bucket,
    integration::cache::{self, Redis},
    store::Store,
};

pub async fn extract_user_id(
    headers: HeaderMap,
    mut request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    match headers.get("user_id") {
        Some(id) => {
            request.extensions_mut().insert(bucket::Id::Protected(
                id.to_str()
                    .map_err(|_| StatusCode::BAD_REQUEST)?
                    .to_string(),
            ));

            Ok(next.run(request).await)
        }
        None => Err(StatusCode::UNAUTHORIZED),
    }
}

const LEASE_SIZE: u128 = 100;

pub async fn lease_tokens(
    Extension(b_id): Extension<bucket::Id>,
    store: State<Arc<Store>>,
    redis: State<Redis>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    if store.check(&b_id) {
        return Ok(next.run(request).await);
    }

    let key = cache::Key::from(&b_id);

    let tokens: Option<u128> = redis.get(&key).await;

    let ttl = match tokens {
        Some(tokens) => {
            if let Some(ttl) = redis.ttl(&key).await {
                redis.set_keep_ttl(&key, tokens - LEASE_SIZE).await;
                Some(ttl)
            } else {
                None
            }
        }
        None => {
            let cfg = store.config();
            let ttl = cfg.protected.reset_in;
            redis
                .set_ex(&key, cfg.protected.quota - LEASE_SIZE, ttl)
                .await;
            Some(ttl)
        }
    };

    if let Some(ttl) = ttl {
        // FIXME
        store.add(b_id, LEASE_SIZE, ttl).unwrap();
    }

    Ok(next.run(request).await)
}
