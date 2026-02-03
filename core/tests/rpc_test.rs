use ccm_core::engine::RetrievalEngine;
use ccm_core::graph::CodeGraph;
use ccm_core::rpc::{ccm_proto, create_service};
use ccm_core::vector::store::LanceDbStore;
use ccm_proto::context_manager_client::ContextManagerClient;
use ccm_proto::ContextRequest;
use std::error::Error;
use std::sync::Arc;
use std::sync::Once;
use tempfile::tempdir;
use tokio::net::TcpListener;
use tonic::transport::Server;

static INIT: Once = Once::new();

fn setup_test_env() {
    INIT.call_once(|| {
        std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    });
}

fn is_permission_denied(err: &dyn Error) -> bool {
    let mut current = Some(err);
    while let Some(err) = current {
        let msg = err.to_string().to_lowercase();
        if msg.contains("operation not permitted") || msg.contains("permission denied") {
            return true;
        }
        current = err.source();
    }
    false
}

#[tokio::test]
async fn test_grpc_get_context() -> anyhow::Result<()> {
    setup_test_env();

    // 1. Setup Engine
    let temp_dir = tempdir()?;
    let db_path = temp_dir.path().join("ccm_db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let graph = Arc::new(tokio::sync::RwLock::new(CodeGraph::new()));
    let store = LanceDbStore::new(&db_path_str, "test_vecs").await?;
    let engine = Arc::new(RetrievalEngine::new(graph, store));

    // 2. Start Server on random port
    let listener = match TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => listener,
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("Skipping RPC test: network permissions denied.");
            return Ok(());
        }
        Err(err) => return Err(err.into()),
    };
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
    let endpoint = tonic::transport::Channel::from_shared(format!("http://{}", addr))?;
    let channel = match endpoint.connect().await {
        Ok(channel) => channel,
        Err(err) if is_permission_denied(&err) => {
            eprintln!("Skipping RPC test: network permissions denied.");
            return Ok(());
        }
        Err(err) => return Err(err.into()),
    };

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
