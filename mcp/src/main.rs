use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

mod protocol;
mod server;
mod tools;

#[tokio::main]
async fn main() -> Result<()> {
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    let env_filter = tracing_subscriber::EnvFilter::try_new(filter)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(std::io::stderr)
        .init();

    // MCP uses stdio for communication
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);

    // Initialize the server state
    let server_state = server::ServerState::new().await?;

    tracing::info!("CCM MCP Server started. Waiting for JSON-RPC messages on stdin...");
    let debug = std::env::var("CCM_MCP_DEBUG")
        .map(|val| matches!(val.to_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false);

    let mut line = String::new();
    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;

        if bytes_read == 0 {
            // EOF reached
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // DEBUG: Log incoming request to stderr
        if debug {
            tracing::debug!(payload = %trimmed, "Received JSON-RPC request");
        }

        // Process the JSON-RPC request
        match server::handle_request(&server_state, trimmed).await {
            Ok(Some(response)) => {
                // Only send response for requests (not notifications)
                let response_str = serde_json::to_string(&response)?;
                if debug {
                    tracing::debug!(payload = %response_str, "Sending JSON-RPC response");
                }
                stdout.write_all(response_str.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
            }
            Ok(None) => {
                // Notification - no response needed
                if debug {
                    tracing::debug!("Notification handled (no response)");
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Error processing request");
                // Send JSON-RPC error response
                let error_response = protocol::create_error_response(
                    None,
                    -32603,
                    &format!("Internal error: {}", e),
                );
                let response_str = serde_json::to_string(&error_response)?;
                stdout.write_all(response_str.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
            }
        }
    }

    Ok(())
}
