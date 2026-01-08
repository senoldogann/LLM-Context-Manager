use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

mod protocol;
mod server;
mod tools;

#[tokio::main]
async fn main() -> Result<()> {
    // MCP uses stdio for communication
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);

    // Initialize the server state
    let server_state = server::ServerState::new().await?;

    eprintln!("CCM MCP Server started. Waiting for JSON-RPC messages on stdin...");

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
        eprintln!("[DEBUG] Received: {}", trimmed);

        // Process the JSON-RPC request
        match server::handle_request(&server_state, trimmed).await {
            Ok(Some(response)) => {
                // Only send response for requests (not notifications)
                let response_str = serde_json::to_string(&response)?;
                eprintln!("[DEBUG] Sending: {}", response_str);
                stdout.write_all(response_str.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
            }
            Ok(None) => {
                // Notification - no response needed
                eprintln!("[DEBUG] Notification handled (no response)");
            }
            Err(e) => {
                eprintln!("Error processing request: {}", e);
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
