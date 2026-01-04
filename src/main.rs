use axum::{Router, routing::get};

mod cfg;
mod handlers;

use handlers::{config_route, root};

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(root))
        .route("/config", get(config_route));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
