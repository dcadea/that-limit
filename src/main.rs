use crate::bootstrap::App;

mod bootstrap;
mod bucket;
mod cfg;
mod error;
mod integration;
mod middleware;
mod state;
mod store;

pub type Result<T> = std::result::Result<T, crate::error::Error>;

#[tokio::main]
async fn main() -> Result<()> {
    let app = App::new().await?;
    app.start().await;
    Ok(())
}
