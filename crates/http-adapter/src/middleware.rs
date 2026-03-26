use axum::{extract::Request, middleware::Next, response::Response};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Bearer},
};
use jsonwebtoken::dangerous;
use log::debug;
use serde::Deserialize;

#[derive(Clone, Deserialize)]
pub struct Claims {
    pub sub: String,
}

pub async fn find_token_claims(
    auth_header: Option<TypedHeader<Authorization<Bearer>>>,
    mut req: Request,
    next: Next,
) -> super::Result<Response> {
    if let Some(token) = auth_header {
        // token should be validated before hitting us
        match dangerous::insecure_decode::<Claims>(token.token()) {
            Ok(token) => {
                req.extensions_mut().insert(token.claims);
            }
            Err(e) => {
                debug!("Failed to decode JWT token: {e:?}");
                return Err(super::Error::InvalidToken);
            }
        }
    }

    Ok(next.run(req).await)
}
