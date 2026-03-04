use std::net::IpAddr;

use axum::{
    extract::FromRequestParts,
    http::{HeaderName, request::Parts},
};
use that_limit_core::BucketId;

pub const X_FORWARDED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");
pub const X_REAL_IP: HeaderName = HeaderName::from_static("x-real-ip");
pub const USER_ID: HeaderName = HeaderName::from_static("user_id");

pub struct Identifier(pub BucketId);

impl<S> FromRequestParts<S> for Identifier
where
    S: Send + Sync,
{
    type Rejection = super::Error;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        if let Some(user_id) = parts.headers.get(&USER_ID).and_then(|v| v.to_str().ok()) {
            return Ok(Self(BucketId::Protected(user_id.into())));
        }

        let ip = parts
            .headers
            .get(&X_FORWARDED_FOR)
            .or_else(|| parts.headers.get(&X_REAL_IP))
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(',').next())
            .and_then(|s| s.parse::<IpAddr>().ok())
            .ok_or(super::Error::Unauthorized)?;

        Ok(Self(BucketId::Public(ip)))
    }
}
