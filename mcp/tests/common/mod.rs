//! `ccm-mcp` sürecini stdio üzerinden süren ortak test bağlayıcısı. Her okuma
//! süreyle sınırlıdır: yanıt vermeyen ya da stdout'u kapatan sunucu CI'ı
//! kilitlemek yerine testi açıklayıcı bir hatayla düşürür.
// Her test dosyası bağlayıcının yalnızca bir kısmını kullanır.
#![allow(dead_code)]

use serde_json::{json, Value};
use std::error::Error;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

pub type TestResult<T> = Result<T, Box<dyn Error>>;

/// Tek bir yanıt için beklenecek en uzun süre.
pub const READ_TIMEOUT: Duration = Duration::from_secs(120);

/// Gerçek `ccm-mcp` sürecini stdio üzerinden süren test bağlayıcısı.
pub struct McpSession {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<std::io::Result<String>>,
    next_id: u64,
    /// Beklenen yanıttan önce gelen mesajlar (eşzamanlı isteklerin yanıtları).
    received: Vec<Value>,
}

impl McpSession {
    /// Sunucuyu başlatır ve `initialize` ile oturumu açar.
    pub fn start(project: &Path, extra_env: &[(&str, &str)]) -> TestResult<Self> {
        let mut session = Self::spawn(project, extra_env)?;
        session.request("initialize", json!({}))?;
        Ok(session)
    }

    /// Sunucuyu stderr günlüğü `log_path`'e yazılacak şekilde (`RUST_LOG=info`)
    /// başlatır ve oturumu açar.
    pub fn start_logged(
        project: &Path,
        extra_env: &[(&str, &str)],
        log_path: &Path,
    ) -> TestResult<Self> {
        let mut env = extra_env.to_vec();
        env.push(("RUST_LOG", "info"));
        let stderr = Stdio::from(std::fs::File::create(log_path)?);
        let mut session = Self::spawn_with_stderr(project, &env, stderr)?;
        session.request("initialize", json!({}))?;
        Ok(session)
    }

    /// Sunucuyu `initialize` göndermeden başlatır.
    pub fn spawn(project: &Path, extra_env: &[(&str, &str)]) -> TestResult<Self> {
        Self::spawn_with_stderr(project, extra_env, Stdio::null())
    }

    fn spawn_with_stderr(
        project: &Path,
        extra_env: &[(&str, &str)],
        stderr: Stdio,
    ) -> TestResult<Self> {
        let mut command = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
        command
            .env("CCM_DISABLE_EMBEDDER", "1")
            .env("CCM_MCP_DEBUG", "0")
            .env("CCM_PROJECT_ROOT", project)
            .env("CCM_ALLOWED_ROOTS", project)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(stderr);
        for (key, value) in extra_env {
            command.env(key, value);
        }
        let mut child = command.spawn()?;
        let stdin = child.stdin.take().ok_or("child stdin missing")?;
        let stdout = child.stdout.take().ok_or("child stdout missing")?;
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || forward_lines(BufReader::new(stdout), sender));
        Ok(Self {
            child,
            stdin,
            lines,
            next_id: 0,
            received: Vec::new(),
        })
    }

    /// Tek satırlık bir JSON-RPC mesajı yazar.
    pub fn send(&mut self, message: &Value) -> TestResult<()> {
        writeln!(self.stdin, "{message}")?;
        self.stdin.flush()?;
        Ok(())
    }

    /// İsteğe yeni bir kimlik verip gönderir; yanıtı beklemez.
    pub fn send_request(&mut self, method: &str, params: Value) -> TestResult<u64> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        Ok(id)
    }

    /// Yanıt beklenmeyen bir bildirim gönderir.
    pub fn notify(&mut self, method: &str, params: Value) -> TestResult<()> {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }

    /// Sunucunun yazdığı sıradaki mesajı en fazla `timeout` bekleyerek okur.
    pub fn read_message(&mut self, timeout: Duration) -> TestResult<Value> {
        match self.lines.recv_timeout(timeout) {
            Ok(Ok(line)) => serde_json::from_str(&line)
                .map_err(|error| format!("server wrote invalid JSON ({error}): {line}").into()),
            Ok(Err(error)) => Err(format!("reading the server's stdout failed: {error}").into()),
            Err(RecvTimeoutError::Timeout) => {
                Err(format!("the server wrote nothing within {timeout:?}").into())
            }
            Err(RecvTimeoutError::Disconnected) => {
                Err("the server closed stdout (EOF) before answering".into())
            }
        }
    }

    /// `id` kimlikli yanıtı en fazla `timeout` bekler; arada gelen diğer
    /// mesajlar sonraki beklemeler için saklanır.
    pub fn wait_for_response(&mut self, id: u64, timeout: Duration) -> TestResult<Value> {
        if let Some(position) = self
            .received
            .iter()
            .position(|message| is_response_to(message, id))
        {
            return Ok(self.received.remove(position));
        }
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let message = self
                .read_message(remaining)
                .map_err(|error| format!("waiting for the response to request {id}: {error}"))?;
            if is_response_to(&message, id) {
                return Ok(message);
            }
            self.received.push(message);
        }
    }

    /// `id` kimlikli yanıt daha önce okunup saklandı mı?
    pub fn has_received(&self, id: u64) -> bool {
        self.received
            .iter()
            .any(|message| is_response_to(message, id))
    }

    /// İsteği gönderir ve yanıtını `READ_TIMEOUT` içinde bekler.
    pub fn request(&mut self, method: &str, params: Value) -> TestResult<Value> {
        let id = self.send_request(method, params)?;
        self.wait_for_response(id, READ_TIMEOUT)
    }

    /// Aracı çağırır ve sonucun ilk metin içeriğini döndürür.
    pub fn call_tool(&mut self, name: &str, arguments: Value) -> TestResult<String> {
        let response = self.request("tools/call", json!({"name": name, "arguments": arguments}))?;
        tool_text(name, &response)
    }
}

impl Drop for McpSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Mesaj, `id` kimlikli isteğin yanıtı mı? Sunucunun istemciye gönderdiği
/// istekler (`method` taşır) yanıt sayılmaz.
fn is_response_to(message: &Value, id: u64) -> bool {
    message.get("method").is_none() && message["id"] == id
}

/// Sunucunun stdout satırlarını kanala aktarır; EOF'ta kanal kapanır.
fn forward_lines(mut reader: BufReader<ChildStdout>, lines: Sender<std::io::Result<String>>) {
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => return,
            Ok(_) => {
                if lines.send(Ok(line)).is_err() {
                    return;
                }
            }
            Err(error) => {
                // Alıcı yoksa oturum bitmiştir; hatayı iletecek kimse kalmaz.
                let _ = lines.send(Err(error));
                return;
            }
        }
    }
}

/// Başarılı araç yanıtının ilk metin içeriği. JSON-RPC hatası, `isError: true`
/// sonucu ya da metin içeriği olmayan yanıt, yanıtın tamamıyla hata olarak döner.
pub fn tool_text(name: &str, response: &Value) -> TestResult<String> {
    if response.get("error").is_some() {
        return Err(format!("{name} returned a JSON-RPC error: {response}").into());
    }
    if response["result"]["isError"] == true {
        return Err(format!("{name} reported a tool error: {response}").into());
    }
    match response["result"]["content"][0]["text"].as_str() {
        Some(text) => Ok(text.to_string()),
        None => Err(format!("{name} response has no text content: {response}").into()),
    }
}

/// `find_nodes` çıktısında sembolün sonuç başlığı olarak bulunup bulunmadığı.
/// "No graph nodes found for query: 'x'" mesajı da sembolü içerdiği için başlık
/// biçimi (`<sembol> (Score:`) aranır.
pub fn found_node(text: &str, symbol: &str) -> bool {
    text.contains(&format!("{symbol} (Score:"))
}

/// Koşul sağlanana kadar `find_nodes` çağırır; süre dolarsa son çıktıyla hata döner.
pub fn poll_find_nodes(
    session: &mut McpSession,
    query: &str,
    deadline: Duration,
    accept: impl Fn(&str) -> bool,
) -> TestResult<String> {
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

/// Ollama `/api/embed` sözleşmesini konuşan deterministik yerel embedding
/// sunucusu; adresini döndürür. Her metin baytlarından türetilen 8 boyutlu bir
/// vektör alır; `rejected_models` içindeki modellerin istekleri HTTP 400 ile
/// reddedilir. Sunucu test süreci bitene kadar çalışır.
pub fn start_embed_server(rejected_models: &'static [&'static str]) -> TestResult<String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let address = format!("http://{}", listener.local_addr()?);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            // MCP sunucusu ve ayrık worker aynı anda istek gönderebilir.
            std::thread::spawn(move || answer_embed_request(stream, rejected_models));
        }
    });
    Ok(address)
}

/// Tek bir `/api/embed` isteğini okuyup yanıtlar; bağlantı ardından kapanır.
fn answer_embed_request(mut stream: std::net::TcpStream, rejected_models: &[&str]) {
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(read_half);
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; content_length];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    let request: Value = serde_json::from_slice(&body).unwrap_or_default();
    let model = request["model"].as_str().unwrap_or_default();
    let (status, payload) = if rejected_models.contains(&model) {
        (
            "400 Bad Request",
            json!({ "error": format!("embedding rejected by the test server for {model}") }),
        )
    } else {
        let embeddings: Vec<Vec<f32>> = request["input"]
            .as_array()
            .map(|inputs| {
                inputs
                    .iter()
                    .map(|input| text_vector(input.as_str().unwrap_or_default()))
                    .collect()
            })
            .unwrap_or_default();
        ("200 OK", json!({ "embeddings": embeddings }))
    };
    let payload = payload.to_string();
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

/// `start_embed_server` sunucusuna bağlı Ollama yapılandırması.
pub fn embedding_env<'a>(host: &'a str, model: &'a str) -> Vec<(&'a str, &'a str)> {
    vec![
        ("CCM_DISABLE_EMBEDDER", "0"),
        ("EMBEDDING_PROVIDER", "ollama"),
        ("EMBEDDING_HOST", host),
        ("EMBEDDING_MODEL", model),
    ]
}

/// Projeyi `model` ile `index_now` üzerinden indeksler ve sunucuyu kapatır.
pub fn index_with_model(project: &Path, host: &str, model: &str) -> TestResult<()> {
    let mut session = McpSession::start(project, &embedding_env(host, model))?;
    session.call_tool("index_now", json!({ "project_path": project }))?;
    Ok(())
}

/// Metnin baytlarından türetilen deterministik 8 boyutlu vektör.
fn text_vector(text: &str) -> Vec<f32> {
    let seed = text.bytes().fold(0u32, |acc, byte| {
        acc.wrapping_mul(31).wrapping_add(u32::from(byte))
    });
    (0..8u32)
        .map(|offset| (seed.wrapping_add(offset) % 97) as f32 / 97.0 + 0.01)
        .collect()
}

/// `ccm-cli index` ile aynı yolu (`update_index`) ayrı bir süreçte çalıştırır:
/// sunucunun dahili worker modu CLI komutuyla aynı çekirdek fonksiyonu çağırır.
pub fn run_update_index_process(project: &Path) -> TestResult<Value> {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"))
        .arg("--ccm-internal-index-worker")
        .arg(project)
        .arg(project.join("data/ccm_db"))
        .env("CCM_INTERNAL_INDEX_WORKER", "1")
        .env("CCM_DISABLE_EMBEDDER", "1")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err(format!("update_index process failed: {}", output.status).into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}
