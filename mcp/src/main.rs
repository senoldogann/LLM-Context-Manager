use anyhow::{Context, Result};
use serde_json::Value;
use std::sync::Arc;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};

mod dispatch;
mod freshness;
mod protocol;
mod server;
mod tools;

const MAX_REQUEST_BYTES: usize = 10 * 1024 * 1024;
const MAX_LOG_BYTES: usize = 16 * 1024;

#[tokio::main]
async fn main() -> Result<()> {
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    let env_filter = tracing_subscriber::EnvFilter::try_new(filter)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(std::io::stderr)
        .init();

    if run_internal_index_worker().await? {
        return Ok(());
    }

    // Allowlist/kök gibi başlangıç ayarları `~/.ccm/.env`'de de tanımlanabilir;
    // ServerState bunları okumadan önce yüklenir (host env'i önceliklidir).
    ccm_core::vector::remote::load_user_env_file()?;

    // Initialize the server state
    let server_state = Arc::new(server::ServerState::new().await?);

    tracing::info!("CCM MCP Server started. Waiting for JSON-RPC messages on stdin...");
    let debug = std::env::var("CCM_MCP_DEBUG")
        .map(|val| matches!(val.to_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false);

    serve_stdio(server_state, debug).await
}

/// MCP stdio oturumu. Okuma döngüsü mesajları sırayla okur: istekler kendi
/// görevlerinde çalışır (yavaş bir araç diğer istekleri bekletmez); bildirimler,
/// istemci yanıtları ve oturumu kuran istekler döngüde sırayla işlenir. Stdout'a
/// yalnızca yazıcı görev yazar. Girdi kapanınca süren isteklerin yanıtları
/// yazılır ve oturum biter.
async fn serve_stdio(state: Arc<server::ServerState>, debug: bool) -> Result<()> {
    let mut reader = BufReader::new(tokio::io::stdin());
    let (outbox, mut writer) = dispatch::start_writer(tokio::io::stdout(), debug);
    let mut dispatcher = dispatch::Dispatcher::new(state.clone(), outbox.clone());

    loop {
        let frame = tokio::select! {
            // Yazıcı yalnızca yazma hatasında durur: yanıt verilemeyen oturum biter.
            stopped = &mut writer => {
                dispatch::writer_result(stopped)?;
                anyhow::bail!("stdout writer stopped before the session ended");
            }
            frame = read_jsonrpc_message(&mut reader, MAX_REQUEST_BYTES) => frame,
        };
        let line = match frame {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(error) if is_recoverable_message_error(&error) => {
                tracing::warn!(error = %error, "Rejected invalid JSON-RPC frame");
                let response = protocol::create_error_response(None, -32700, "Parse error");
                outbox.send(&response).await?;
                continue;
            }
            Err(error) => return Err(error),
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if debug {
            tracing::debug!(payload = %sanitize_payload(trimmed), "Received JSON-RPC message");
        }

        match server::classify_message(trimmed) {
            server::IncomingMessage::Rejected(response) => outbox.send(&response).await?,
            server::IncomingMessage::Dropped => {}
            server::IncomingMessage::ClientResponse(response) => {
                state.handle_client_response(&response)
            }
            server::IncomingMessage::Notification(notification)
                if notification.method == "notifications/cancelled" =>
            {
                dispatcher.cancel(notification.params.as_ref())
            }
            server::IncomingMessage::Notification(notification) => {
                server::handle_notification(&state, &notification);
                send_server_requests(&state, &outbox).await?;
            }
            server::IncomingMessage::Request(request)
                if server::is_session_request(&request.method) =>
            {
                let response = server::handle_request(&state, request).await;
                // Sunucudan istemciye istekler (ör. roots/list) yanıttan önce gönderilir.
                send_server_requests(&state, &outbox).await?;
                outbox.send(&response).await?;
            }
            server::IncomingMessage::Request(request) => dispatcher.dispatch(request),
        }
    }

    dispatcher.drain().await;
    // Son `Outbox` kopyası bırakılınca yazıcı kuyruğu boşaltıp kapanır.
    drop(outbox);
    dispatch::writer_result(writer.await)
}

/// Sunucunun istemciye göndermek üzere kuyruğa aldığı istekleri (ör.
/// `roots/list`) yazıcıya iletir.
async fn send_server_requests(
    state: &server::ServerState,
    outbox: &dispatch::Outbox,
) -> Result<()> {
    for request in state.take_outgoing_requests() {
        outbox.send(&request).await?;
    }
    Ok(())
}

async fn run_internal_index_worker() -> Result<bool> {
    const INTERNAL_COMMAND: &str = "--ccm-internal-index-worker";
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some(INTERNAL_COMMAND) {
        return Ok(false);
    }
    if std::env::var("CCM_INTERNAL_INDEX_WORKER").as_deref() != Ok("1") {
        anyhow::bail!("Internal index worker mode requires CCM_INTERNAL_INDEX_WORKER=1");
    }
    let project_path = args
        .next()
        .context("Internal worker project path is missing")?;
    let db_path = args.next().context("Internal worker DB path is missing")?;
    let mode = args.next();
    if args.next().is_some() {
        anyhow::bail!("Internal index worker received unexpected arguments");
    }
    if let Ok(delay) = std::env::var("CCM_INTERNAL_INDEX_TEST_DELAY_MS") {
        let delay_ms = delay
            .parse::<u64>()
            .context("CCM_INTERNAL_INDEX_TEST_DELAY_MS must be an integer")?;
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
    }
    let stats = match mode.as_deref() {
        Some("quick") => {
            ccm_core::index_directory_with_mode(
                &project_path,
                Some(&db_path),
                ccm_core::IndexMode::Quick,
            )
            .await?
        }
        Some("upgrade") => {
            ccm_core::upgrade_active_index_semantics(&project_path, Some(&db_path)).await?
        }
        _ => ccm_core::update_index(&project_path, Some(&db_path)).await?,
    };
    println!("{}", serde_json::to_string(&stats)?);
    Ok(true)
}

fn is_recoverable_message_error(error: &anyhow::Error) -> bool {
    let message = error.to_string();
    message.starts_with("JSON-RPC request exceeds")
        || message.starts_with("JSON-RPC request is not valid UTF-8")
}

async fn read_jsonrpc_message<R>(reader: &mut R, max_bytes: usize) -> Result<Option<String>>
where
    R: AsyncBufRead + Unpin,
{
    let mut bytes = Vec::new();

    loop {
        let (chunk, hit_newline) = {
            let buffer = reader.fill_buf().await?;
            if buffer.is_empty() {
                if bytes.is_empty() {
                    return Ok(None);
                }
                break;
            }

            let consume_len = match buffer.iter().position(|byte| *byte == b'\n') {
                Some(position) => position + 1,
                None => buffer.len(),
            };

            (
                buffer[..consume_len].to_vec(),
                buffer[..consume_len].ends_with(b"\n"),
            )
        };

        if bytes.len() + chunk.len() > max_bytes {
            reader.consume(chunk.len());
            if !hit_newline {
                discard_until_newline(reader).await?;
            }
            return Err(anyhow::anyhow!(
                "JSON-RPC request exceeds {} bytes",
                max_bytes
            ));
        }

        bytes.extend_from_slice(&chunk);
        reader.consume(chunk.len());

        if hit_newline {
            break;
        }
    }

    String::from_utf8(bytes)
        .map(Some)
        .context("JSON-RPC request is not valid UTF-8")
}

async fn discard_until_newline<R>(reader: &mut R) -> Result<bool>
where
    R: AsyncBufRead + Unpin,
{
    loop {
        let (consume_len, hit_newline) = {
            let buffer = reader.fill_buf().await?;
            if buffer.is_empty() {
                return Ok(false);
            }

            match buffer.iter().position(|byte| *byte == b'\n') {
                Some(position) => (position + 1, true),
                None => (buffer.len(), false),
            }
        };

        reader.consume(consume_len);
        if hit_newline {
            return Ok(true);
        }
    }
}

fn sanitize_payload(payload: &str) -> String {
    let sanitized = match serde_json::from_str::<Value>(payload) {
        Ok(mut value) => {
            redact_json_value(&mut value);
            serde_json::to_string(&value).unwrap_or_else(|_| redact_inline_secret(payload))
        }
        Err(_) => redact_inline_secret(payload),
    };

    truncate_for_log(&sanitized, MAX_LOG_BYTES)
}

fn redact_json_value(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, entry) in map.iter_mut() {
                if is_sensitive_key(key) {
                    *entry = Value::String("[REDACTED]".to_string());
                } else {
                    redact_json_value(entry);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_json_value(item);
            }
        }
        Value::String(text) => {
            *text = redact_inline_secret(text);
        }
        _ => {}
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.contains("authorization")
        || lower.contains("api_key")
        || lower.ends_with("_key")
        || lower.contains("token")
        || lower.contains("secret")
}

fn redact_inline_secret(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    if lower.contains("bearer ") {
        return "[REDACTED]".to_string();
    }

    text.to_string()
}

fn truncate_for_log(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }

    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }

    format!("{}...[truncated]", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::{read_jsonrpc_message, sanitize_payload};
    use tokio::io::BufReader;

    #[tokio::test]
    async fn oversized_request_is_rejected() {
        let data = format!("{}\n", "a".repeat(32));
        let mut reader = BufReader::new(data.as_bytes());
        let error = read_jsonrpc_message(&mut reader, 16).await.unwrap_err();
        assert!(error.to_string().contains("exceeds 16 bytes"));
    }

    #[test]
    fn sanitize_payload_redacts_sensitive_fields() {
        let payload = r#"{"headers":{"Authorization":"Bearer secret-token"},"api_key":"abc"}"#;
        let sanitized = sanitize_payload(payload);
        assert!(sanitized.contains("[REDACTED]"));
        assert!(!sanitized.contains("secret-token"));
        assert!(!sanitized.contains("\"abc\""));
    }
}
