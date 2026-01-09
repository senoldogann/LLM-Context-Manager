use crate::engine::RetrievalEngine;
use crate::graph::CodeGraph;
use crate::rpc;
use crate::vector::store::LanceDbStore;
use axum::{routing::get, Router};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::try_join;
use tonic::transport::Server;

/// Starts the CCM Core servers (HTTP and gRPC).
///
/// # Arguments
/// * `http_port` - The port number for the HTTP server (Health Check)
pub async fn start_server(http_port: u16) -> anyhow::Result<()> {
    // Shared State Initialization
    println!("Initializing CCM Core Engine...");
    let graph = CodeGraph::new();
    // Use a persistent path for the vector store
    let store = LanceDbStore::new("data/ccm_db", "code_vectors").await?;
    let engine = Arc::new(RetrievalEngine::new(Arc::new(graph), store));

    // 1. HTTP Server (Health Check & Debug)
    let app = Router::new().route("/", get(|| async { "CCM Core is Running" }));
    let http_addr = SocketAddr::from(([127, 0, 0, 1], http_port));
    println!("HTTP Server listening on {}", http_addr);

    let http_server = axum::serve(tokio::net::TcpListener::bind(http_addr).await?, app);

    // 2. gRPC Server (Main Data Channel)
    let grpc_addr = "[::1]:50051".parse()?;
    println!("gRPC Server listening on {}", grpc_addr);

    let grpc_service = rpc::create_service(engine.clone());
    let grpc_server = Server::builder().add_service(grpc_service).serve(grpc_addr);

    // Run both concurrently
    println!("CCM Sidecar is ready to accept connections.");

    try_join!(
        async move {
            http_server
                .await
                .map_err(|e| anyhow::anyhow!("HTTP Server error: {}", e))
        },
        async move {
            grpc_server
                .await
                .map_err(|e| anyhow::anyhow!("gRPC Server error: {}", e))
        }
    )?;

    Ok(())
}
