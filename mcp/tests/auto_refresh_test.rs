use serde_json::{json, Value};
use std::error::Error;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};
use tempfile::tempdir;

/// Gerçek `ccm-mcp` sürecini stdio üzerinden süren test bağlayıcısı.
struct McpSession {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    next_id: u64,
}

impl McpSession {
    fn start(project: &Path, extra_env: &[(&str, &str)]) -> Result<Self, Box<dyn Error>> {
        let mut command = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
        command
            .env("CCM_DISABLE_EMBEDDER", "1")
            .env("CCM_MCP_DEBUG", "0")
            .env("CCM_PROJECT_ROOT", project)
            .env("CCM_ALLOWED_ROOTS", project)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (key, value) in extra_env {
            command.env(key, value);
        }
        let mut child = command.spawn()?;
        let stdin = child.stdin.take().ok_or("child stdin missing")?;
        let reader = BufReader::new(child.stdout.take().ok_or("child stdout missing")?);
        let mut session = Self {
            child,
            stdin,
            reader,
            next_id: 0,
        };
        session.request("initialize", json!({}))?;
        Ok(session)
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, Box<dyn Error>> {
        self.next_id += 1;
        let message =
            json!({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params});
        writeln!(self.stdin, "{}", message)?;
        self.stdin.flush()?;
        let mut line = String::new();
        self.reader.read_line(&mut line)?;
        Ok(serde_json::from_str(&line)?)
    }

    fn call_tool(&mut self, name: &str, arguments: Value) -> Result<String, Box<dyn Error>> {
        let response = self.request("tools/call", json!({"name": name, "arguments": arguments}))?;
        if let Some(error) = response.get("error") {
            return Err(format!("{name} returned a JSON-RPC error: {error}").into());
        }
        Ok(response["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string())
    }
}

impl Drop for McpSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `find_nodes` çıktısında sembolün sonuç başlığı olarak bulunup bulunmadığı.
/// "No graph nodes found for query: 'x'" mesajı da sembolü içerdiği için başlık
/// biçimi (`<sembol> (Score:`) aranır.
fn found_node(text: &str, symbol: &str) -> bool {
    text.contains(&format!("{symbol} (Score:"))
}

/// Koşul sağlanana kadar `find_nodes` çağırır; süre dolarsa son çıktıyla hata döner.
// Sonraki otomatik yenileme senaryoları için ortak yardımcı; bu dosyada henüz çağrılmıyor.
#[allow(dead_code)]
fn poll_find_nodes(
    session: &mut McpSession,
    query: &str,
    deadline: Duration,
    accept: impl Fn(&str) -> bool,
) -> Result<String, Box<dyn Error>> {
    let started = Instant::now();
    loop {
        let text = session.call_tool("find_nodes", json!({ "query": query }))?;
        if accept(&text) {
            return Ok(text);
        }
        if started.elapsed() > deadline {
            return Err(
                format!("condition not met within {deadline:?}; last output: {text}").into(),
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn freshness_line_reports_disabled_auto_refresh_and_index_age() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn tracked_symbol() {}\n")?;
    let mut session = McpSession::start(project.path(), &[("CCM_AUTO_REFRESH", "0")])?;
    session.call_tool("index_now", json!({ "project_path": project.path() }))?;

    let text = session.call_tool("find_nodes", json!({ "query": "tracked_symbol" }))?;

    assert!(
        text.starts_with("_Index: auto-refresh off · indexed "),
        "unexpected freshness line: {text}"
    );
    assert!(found_node(&text, "tracked_symbol"));
    Ok(())
}
