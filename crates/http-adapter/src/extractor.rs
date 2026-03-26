use std::net::IpAddr;

use axum::{
    extract::FromRequestParts,
    http::{HeaderName, request::Parts},
};
use that_limit_core::BucketId;

use crate::middleware::Claims;

pub const X_FORWARDED_HOST: HeaderName = HeaderName::from_static("x-forwarded-host");
pub const X_FORWARDED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");
pub const X_REAL_IP: HeaderName = HeaderName::from_static("x-real-ip");

pub struct Host(pub String);

impl Host {
    pub fn domain(&self) -> Option<&str> {
        self.hostname().split('.').next()
    }

    fn hostname(&self) -> &str {
        if self.0.starts_with('[') {
            self.0.split(']').next().map_or(&self.0, |s| &s[1..])
        } else {
            self.0.split(':').next().unwrap_or(&self.0)
        }
    }
}

impl<S> FromRequestParts<S> for Host
where
    S: Send + Sync,
{
    type Rejection = super::Error;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .headers
            .get(&X_FORWARDED_HOST)
            .filter(|h| !h.is_empty())
            .and_then(|h| h.to_str().ok())
            .map(|h| Self(h.to_string()))
            .ok_or(super::Error::MissingHost)
    }
}

pub struct Identifier(pub BucketId);

impl<S> FromRequestParts<S> for Identifier
where
    S: Send + Sync,
{
    type Rejection = super::Error;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        if let Some(claims) = parts.extensions.get::<Claims>() {
            return Ok(Self(BucketId::Protected(claims.sub.as_str().into())));
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
