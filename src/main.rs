use axum::{Router, routing::get};

mod cfg;

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(cfg::handler::root))
        .route("/config", get(cfg::handler::get));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
