use crate::bootstrap::App;

mod bootstrap;
mod core;
mod error;
#[cfg(feature = "grpc")]
mod grpc;
#[cfg(feature = "http")]
mod http;

#[cfg(not(any(feature = "grpc", feature = "http")))]
compile_error!("Either feature `grpc` or `http` must be enabled.");

pub type Result<T> = std::result::Result<T, crate::error::Error>;

#[tokio::main]
async fn main() -> Result<()> {
    let app = App::new().await?;
    app.run().await;
    Ok(())
}
