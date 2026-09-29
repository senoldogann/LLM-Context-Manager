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

#[test]
fn saved_change_becomes_searchable_without_manual_index() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn existing_symbol() {}\n")?;
    let mut session = McpSession::start(project.path(), &[])?;
    session.call_tool("index_now", json!({ "project_path": project.path() }))?;

    fs::write(
        project.path().join("added.rs"),
        "fn freshly_saved_symbol() {}\n",
    )?;
    let text = poll_find_nodes(
        &mut session,
        "freshly_saved_symbol",
        Duration::from_secs(10),
        |text| found_node(text, "freshly_saved_symbol"),
    )?;

    assert!(
        text.starts_with("_Index: fresh · auto-refresh on_"),
        "unexpected freshness line: {text}"
    );
    Ok(())
}

#[test]
fn slow_refresh_returns_stale_result_within_budget() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn existing_symbol() {}\n")?;
    let mut session = McpSession::start(
        project.path(),
        &[("CCM_INTERNAL_INDEX_TEST_DELAY_MS", "5000")],
    )?;
    session.call_tool("index_now", json!({ "project_path": project.path() }))?;

    // index_now sonrası başlangıç yakalaması da 5 sn sürer; kaydedilen değişiklik beklemede kalır.
    fs::write(project.path().join("added.rs"), "fn pending_symbol() {}\n")?;
    std::thread::sleep(Duration::from_millis(500));
    let started = Instant::now();
    let text = session.call_tool("find_nodes", json!({ "query": "existing_symbol" }))?;

    assert!(
        started.elapsed() < Duration::from_secs(4),
        "read exceeded the 2 s budget: {:?}",
        started.elapsed()
    );
    assert!(
        text.starts_with("_Index: stale · "),
        "unexpected freshness line: {text}"
    );
    assert!(
        text.contains("refresh running"),
        "unexpected freshness line: {text}"
    );
    assert!(found_node(&text, "existing_symbol"));
    Ok(())
}

#[test]
fn disabled_auto_refresh_keeps_manual_semantics() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn existing_symbol() {}\n")?;
    let mut session = McpSession::start(project.path(), &[("CCM_AUTO_REFRESH", "0")])?;
    session.call_tool("index_now", json!({ "project_path": project.path() }))?;

    fs::write(
        project.path().join("added.rs"),
        "fn unindexed_symbol() {}\n",
    )?;
    std::thread::sleep(Duration::from_millis(1_500));
    let text = session.call_tool("find_nodes", json!({ "query": "unindexed_symbol" }))?;

    assert!(
        !found_node(&text, "unindexed_symbol"),
        "auto-refresh must stay off: {text}"
    );
    assert!(
        text.starts_with("_Index: auto-refresh off"),
        "unexpected freshness line: {text}"
    );
    Ok(())
}

#[test]
fn ignored_and_artifact_writes_do_not_mark_index_stale() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    // `.gitignore` yalnızca git deposunda uygulanır (tarama ve izleme filtresi aynı
    // kuralı izler); boş bir `.git` dizini ikisi için de depo işaretidir.
    fs::create_dir(project.path().join(".git"))?;
    fs::write(project.path().join(".gitignore"), "generated/\n")?;
    fs::write(project.path().join("main.rs"), "fn existing_symbol() {}\n")?;
    let mut session = McpSession::start(
        project.path(),
        &[("CCM_INTERNAL_INDEX_TEST_DELAY_MS", "3000")],
    )?;
    session.call_tool("index_now", json!({ "project_path": project.path() }))?;
    poll_find_nodes(
        &mut session,
        "existing_symbol",
        Duration::from_secs(15),
        |text| text.starts_with("_Index: fresh"),
    )?;

    // Gerçek değişiklik yeni generation yazar; indeksin kendi dosyaları ikinci
    // bir (3 sn'lik) yenilemeyi tetiklerse hemen sonraki okuma bayat görünür.
    fs::write(
        project.path().join("added.rs"),
        "fn real_change_symbol() {}\n",
    )?;
    poll_find_nodes(
        &mut session,
        "real_change_symbol",
        Duration::from_secs(15),
        |text| found_node(text, "real_change_symbol") && text.starts_with("_Index: fresh"),
    )?;
    std::thread::sleep(Duration::from_millis(800));
    let started = Instant::now();
    let after_refresh = session.call_tool("find_nodes", json!({ "query": "existing_symbol" }))?;
    assert!(
        after_refresh.starts_with("_Index: fresh"),
        "artifact writes re-triggered: {after_refresh}"
    );
    assert!(started.elapsed() < Duration::from_secs(1));

    fs::create_dir_all(project.path().join("generated"))?;
    fs::write(
        project.path().join("generated/out.rs"),
        "fn generated_symbol() {}\n",
    )?;
    std::thread::sleep(Duration::from_millis(800));
    let started = Instant::now();
    let after_ignored = session.call_tool("find_nodes", json!({ "query": "existing_symbol" }))?;
    assert!(
        after_ignored.starts_with("_Index: fresh"),
        "ignored write triggered: {after_ignored}"
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    Ok(())
}

#[test]
fn atomic_rename_save_is_picked_up() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn before_save() {}\n")?;
    let mut session = McpSession::start(project.path(), &[])?;
    session.call_tool("index_now", json!({ "project_path": project.path() }))?;

    let temp = project.path().join(".main.rs.swp-save");
    fs::write(&temp, "fn after_atomic_save() {}\n")?;
    fs::rename(&temp, project.path().join("main.rs"))?;

    poll_find_nodes(
        &mut session,
        "after_atomic_save",
        Duration::from_secs(10),
        |text| found_node(text, "after_atomic_save") && text.starts_with("_Index: fresh"),
    )?;
    let old = session.call_tool("find_nodes", json!({ "query": "before_save" }))?;
    assert!(
        !found_node(&old, "before_save"),
        "old symbol must be gone: {old}"
    );
    Ok(())
}

#[test]
fn bulk_change_coalesces_and_settles() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn existing_symbol() {}\n")?;
    let mut session = McpSession::start(project.path(), &[])?;
    session.call_tool("index_now", json!({ "project_path": project.path() }))?;

    for index in 0..200 {
        fs::write(
            project.path().join(format!("bulk_{index}.rs")),
            format!("fn bulk_symbol_{index}() {{}}\n"),
        )?;
    }

    poll_find_nodes(
        &mut session,
        "bulk_symbol_199",
        Duration::from_secs(30),
        |text| found_node(text, "bulk_symbol_199") && text.starts_with("_Index: fresh"),
    )?;
    let first = session.call_tool("find_nodes", json!({ "query": "bulk_symbol_0" }))?;
    assert!(found_node(&first, "bulk_symbol_0"), "{first}");
    Ok(())
}

#[test]
fn graph_only_refresh_reports_semantic_notice() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn existing_symbol() {}\n")?;
    let mut session = McpSession::start(
        project.path(),
        &[
            ("CCM_DISABLE_EMBEDDER", "0"),
            ("EMBEDDING_HOST", "http://127.0.0.1:9"),
            ("EMBEDDING_TIMEOUT_SECS", "2"),
        ],
    )?;
    session.call_tool("index_now", json!({ "project_path": project.path() }))?;

    fs::write(
        project.path().join("added.rs"),
        "fn graph_only_symbol() {}\n",
    )?;
    let text = poll_find_nodes(
        &mut session,
        "graph_only_symbol",
        Duration::from_secs(20),
        |text| found_node(text, "graph_only_symbol"),
    )?;

    assert!(
        text.contains("semantic search unavailable"),
        "unexpected freshness line: {text}"
    );
    Ok(())
}

#[test]
fn quick_index_upgrade_defers_refresh_without_blocking_reads() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn existing_symbol() {}\n")?;
    // Gecikme hem hızlı indeksi hem de ayrık semantik yükseltme sürecini uzatır.
    let mut session = McpSession::start(
        project.path(),
        &[("CCM_INTERNAL_INDEX_TEST_DELAY_MS", "3000")],
    )?;
    session.call_tool(
        "index_now",
        json!({ "project_path": project.path(), "mode": "quick" }),
    )?;

    fs::write(
        project.path().join("added.rs"),
        "fn during_upgrade_symbol() {}\n",
    )?;
    std::thread::sleep(Duration::from_millis(800));
    let started = Instant::now();
    let text = session.call_tool("find_nodes", json!({ "query": "existing_symbol" }))?;
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "reads must not wait while the upgrade defers the refresh: {:?}",
        started.elapsed()
    );
    assert!(
        text.contains("waiting for semantic upgrade"),
        "unexpected freshness line: {text}"
    );

    poll_find_nodes(
        &mut session,
        "during_upgrade_symbol",
        Duration::from_secs(20),
        |text| found_node(text, "during_upgrade_symbol") && text.starts_with("_Index: fresh"),
    )?;
    Ok(())
}

#[test]
fn manual_index_during_auto_refresh_succeeds() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn existing_symbol() {}\n")?;
    let mut session = McpSession::start(project.path(), &[])?;
    session.call_tool("index_now", json!({ "project_path": project.path() }))?;

    fs::write(project.path().join("added.rs"), "fn raced_symbol() {}\n")?;
    let manual = session.call_tool("index_now", json!({ "project_path": project.path() }))?;
    assert!(
        manual.contains("Project index refreshed successfully")
            || manual.contains("already up to date"),
        "manual index must succeed next to auto-refresh: {manual}"
    );

    poll_find_nodes(
        &mut session,
        "raced_symbol",
        Duration::from_secs(10),
        |text| found_node(text, "raced_symbol") && text.starts_with("_Index: fresh"),
    )?;
    Ok(())
}
