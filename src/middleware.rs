use axum::{
    extract::Request,
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};

#[derive(Clone)]
pub struct UserId(pub String);

pub async fn extract_user_id(
    headers: HeaderMap,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let user_id = headers.get("user_id");

    match user_id {
        Some(id) => {
            request.extensions_mut().insert(UserId(
                id.to_str()
                    .map_err(|_| StatusCode::BAD_REQUEST)?
                    .to_string(),
            ));

            Ok(next.run(request).await)
        }
        None => Err(StatusCode::UNAUTHORIZED),
    }
}
