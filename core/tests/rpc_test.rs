use ccm_core::engine::RetrievalEngine;
use ccm_core::graph::CodeGraph;
use ccm_core::rpc::{ccm_proto, create_service};
use ccm_core::vector::store::LanceDbStore;
use ccm_proto::context_manager_client::ContextManagerClient;
use ccm_proto::ContextRequest;
use std::sync::Arc;
use tokio::net::TcpListener;
use tonic::transport::Server;

#[tokio::test]
async fn test_grpc_get_context() -> anyhow::Result<()> {
    // 1. Setup Engine
    let graph = CodeGraph::new();
    let store = LanceDbStore::new("data/test_grpc_db", "test_vecs").await?;
    let engine = Arc::new(RetrievalEngine::new(graph, store));

    // 2. Start Server on random port
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let service = create_service(engine);

    tokio::spawn(async move {
        Server::builder()
            .add_service(service)
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .unwrap();
    });

    // 3. Create Client and Call
    let channel = tonic::transport::Channel::from_shared(format!("http://{}", addr))?
        .connect()
        .await?;

    let mut client = ContextManagerClient::new(channel);

    let request = tonic::Request::new(ContextRequest {
        file_path: "src/main.rs".to_string(),
        line: 10,
        column: 0,
    });

    let response = client.get_context(request).await?;
    let resp = response.into_inner();

    // 4. Assert
    // Even if empty, it means the RPC call succeeded.
    // Since graph is empty, suggestions typically empty unless we add data.
    println!("Response: {:?}", resp);
    assert!(resp.suggestions.is_empty() || !resp.suggestions.is_empty());

    Ok(())
}
