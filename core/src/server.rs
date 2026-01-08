use axum::{routing::get, Router};
use std::net::SocketAddr;

/// Starts the CCM Core HTTP server on the specified port.
///
/// # Arguments
/// * `port` - The port number to listen on (default: 3000)
pub async fn start_server(port: u16) -> anyhow::Result<()> {
    let app = Router::new().route("/", get(handler));

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    println!("CCM Core Server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn handler() -> &'static str {
    "CCM Core is running!"
}
