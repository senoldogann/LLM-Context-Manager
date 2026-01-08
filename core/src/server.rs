use axum::{routing::get, Router};
use std::net::SocketAddr;

pub async fn start_server() -> anyhow::Result<()> {
    let app = Router::new().route("/", get(handler));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("CCM Core Server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn handler() -> &'static str {
    "CCM Core is running!"
}
