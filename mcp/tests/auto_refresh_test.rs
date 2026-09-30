use serde_json::{json, Value};
use std::error::Error;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
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

/// Etkin generation dizini (`data/.ccm-generations/<işaretçi>`).
fn active_generation_dir(project: &Path) -> Result<std::path::PathBuf, Box<dyn Error>> {
    let generation = fs::read_to_string(project.join("data/ccm_current"))?;
    Ok(project
        .join("data/.ccm-generations")
        .join(generation.trim()))
}

/// Etkin generation'ın diskteki manifesti `file_id`'yi içerene (canlı yenileme
/// kalıcılaştırılana) kadar bekler.
fn wait_for_persisted_file(
    project: &Path,
    file_id: &str,
    deadline: Duration,
) -> Result<(), Box<dyn Error>> {
    let started = Instant::now();
    loop {
        let manifest_path = active_generation_dir(project)?.join("ccm_manifest.json");
        let manifest: Value = serde_json::from_slice(&fs::read(&manifest_path)?)?;
        if manifest["files"].get(file_id).is_some() {
            return Ok(());
        }
        if started.elapsed() > deadline {
            return Err(format!("'{file_id}' was not persisted within {deadline:?}").into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// `ccm-cli index` ile aynı yolu (`update_index`) ayrı bir süreçte çalıştırır:
/// sunucunun dahili worker modu CLI komutuyla aynı çekirdek fonksiyonu çağırır.
fn run_update_index_process(project: &Path) -> Result<Value, Box<dyn Error>> {
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
        &[("CCM_INTERNAL_REFRESH_TEST_DELAY_MS", "5000")],
    )?;
    session.call_tool("index_now", json!({ "project_path": project.path() }))?;

    // index_now sonrası başlangıç yakalaması 5 sn sürer; kaydedilen değişiklik beklemede kalır.
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
    // kuralı izler); boş bir `.git` dizini ikisi için de depo işaretidir. Geçerli bir
    // depo olmadığı için indeks `.git/info/exclude` yazmaz; indeksin kendi
    // dosyalarına karşı korumayı yalnızca izleme filtresinin kendi kuralları sağlar.
    fs::create_dir(project.path().join(".git"))?;
    fs::write(project.path().join(".gitignore"), "generated/\n")?;
    fs::write(project.path().join("main.rs"), "fn existing_symbol() {}\n")?;
    // Her yenileme turu 3 sn sürer; indeksin kendi yazımları ikinci bir tur
    // tetiklerse okumalar bu süre boyunca `refresh running` görür.
    let mut session = McpSession::start(
        project.path(),
        &[("CCM_INTERNAL_REFRESH_TEST_DELAY_MS", "3000")],
    )?;
    session.call_tool("index_now", json!({ "project_path": project.path() }))?;
    poll_find_nodes(
        &mut session,
        "existing_symbol",
        Duration::from_secs(15),
        |text| text.starts_with("_Index: fresh"),
    )?;

    // Gerçek değişiklik canlı engine'e uygulanır, ardından graf ve manifest etkin
    // generation dizinine (geçici dosya + rename ile) yazılır. Bu yazımlar yeni bir
    // tur tetiklememelidir.
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
    wait_for_persisted_file(project.path(), "./added.rs", Duration::from_secs(15))?;
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

#[test]
fn continuous_events_do_not_postpone_the_refresh_forever() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn existing_symbol() {}\n")?;
    let mut session = McpSession::start(project.path(), &[])?;
    session.call_tool("index_now", json!({ "project_path": project.path() }))?;
    poll_find_nodes(
        &mut session,
        "existing_symbol",
        Duration::from_secs(15),
        |text| text.starts_with("_Index: fresh"),
    )?;

    // Bir yazıcı 100 ms aralıkla aynı dosyaya yazar; 300 ms'lik sessizlik penceresi
    // hiç dolmaz. Debounce'un üst sınırı olmazsa yenileme yazıcı durana kadar başlamaz.
    let noisy = project.path().join("noisy.rs");
    let stop = Arc::new(AtomicBool::new(false));
    let writer = std::thread::spawn({
        let stop = stop.clone();
        move || -> std::io::Result<()> {
            let mut tick = 0_u32;
            while !stop.load(Ordering::SeqCst) {
                tick += 1;
                fs::write(&noisy, format!("fn noisy_symbol() {{}}\n// tick {tick}\n"))?;
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(())
        }
    });
    let found = poll_find_nodes(
        &mut session,
        "noisy_symbol",
        Duration::from_secs(8),
        |text| found_node(text, "noisy_symbol"),
    );
    stop.store(true, Ordering::SeqCst);
    writer.join().map_err(|_| "writer thread panicked")??;
    found?;
    Ok(())
}

#[test]
fn removed_index_is_not_rebuilt_by_auto_refresh() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn existing_symbol() {}\n")?;
    let mut session = McpSession::start(project.path(), &[])?;
    session.call_tool("index_now", json!({ "project_path": project.path() }))?;
    poll_find_nodes(
        &mut session,
        "existing_symbol",
        Duration::from_secs(15),
        |text| text.starts_with("_Index: fresh"),
    )?;

    // Kullanıcı indeksi siler ve izlenen bir dosyayı değiştirir. Otomatik yenileme
    // indekslenmemiş bir projeyi kendiliğinden yeniden indekslememelidir.
    fs::remove_dir_all(project.path().join("data"))?;
    fs::write(
        project.path().join("added.rs"),
        "fn after_removal_symbol() {}\n",
    )?;
    std::thread::sleep(Duration::from_millis(3_000));

    assert!(
        !project.path().join("data/ccm_current").exists(),
        "auto-refresh must not rebuild a removed index"
    );
    let error = session
        .call_tool("find_nodes", json!({ "query": "after_removal_symbol" }))
        .expect_err("reads must report the missing index");
    assert!(
        error.to_string().contains("Project index is missing"),
        "unexpected read error: {error}"
    );
    Ok(())
}

#[test]
fn live_refresh_does_not_wait_for_the_index_worker() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn existing_symbol() {}\n")?;
    // Worker süreci 5 sn gecikir: elle indeksleme bunu öder, otomatik yenileme ödememelidir.
    let mut session = McpSession::start(
        project.path(),
        &[("CCM_INTERNAL_INDEX_TEST_DELAY_MS", "5000")],
    )?;
    session.call_tool("index_now", json!({ "project_path": project.path() }))?;
    poll_find_nodes(
        &mut session,
        "existing_symbol",
        Duration::from_secs(10),
        |text| text.starts_with("_Index: fresh"),
    )?;

    let saved_at = Instant::now();
    fs::write(project.path().join("added.rs"), "fn live_symbol() {}\n")?;
    poll_find_nodes(
        &mut session,
        "live_symbol",
        Duration::from_secs(10),
        |text| found_node(text, "live_symbol") && text.starts_with("_Index: fresh"),
    )?;
    assert!(
        saved_at.elapsed() < Duration::from_secs(2),
        "the refresh took {:?}; a worker process (5 s delay) must not be involved",
        saved_at.elapsed()
    );
    Ok(())
}

#[test]
fn live_refresh_is_persisted_and_a_skipped_write_is_reconciled() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn existing_symbol() {}\n")?;
    let mut writer = McpSession::start(project.path(), &[])?;
    writer.call_tool("index_now", json!({ "project_path": project.path() }))?;
    poll_find_nodes(
        &mut writer,
        "existing_symbol",
        Duration::from_secs(10),
        |text| text.starts_with("_Index: fresh"),
    )?;
    let generation = active_generation_dir(project.path())?;
    let graph_before = fs::read(generation.join("ccm_graph.json"))?;
    let manifest_before = fs::read(generation.join("ccm_manifest.json"))?;

    fs::write(
        project.path().join("added.rs"),
        "fn persisted_symbol() {}\n",
    )?;
    poll_find_nodes(
        &mut writer,
        "persisted_symbol",
        Duration::from_secs(10),
        |text| found_node(text, "persisted_symbol") && text.starts_with("_Index: fresh"),
    )?;
    wait_for_persisted_file(project.path(), "./added.rs", Duration::from_secs(10))?;
    drop(writer);
    assert_eq!(
        active_generation_dir(project.path())?,
        generation,
        "the live refresh must persist into the active generation"
    );

    // Otomatik yenilemesi kapalı yeni süreç değişikliği yalnızca diskten görebilir.
    let mut reader = McpSession::start(project.path(), &[("CCM_AUTO_REFRESH", "0")])?;
    let text = reader.call_tool("find_nodes", json!({ "query": "persisted_symbol" }))?;
    assert!(
        found_node(&text, "persisted_symbol"),
        "a new server must read the persisted change: {text}"
    );
    drop(reader);

    // Kalıcılaştırma hiç olmamış gibi eski graf ve manifest geri konur; yeni
    // sunucunun başlangıç karşılaştırması farkı yakalar.
    fs::write(generation.join("ccm_graph.json"), &graph_before)?;
    fs::write(generation.join("ccm_manifest.json"), &manifest_before)?;
    let mut recovered = McpSession::start(project.path(), &[])?;
    poll_find_nodes(
        &mut recovered,
        "persisted_symbol",
        Duration::from_secs(10),
        |text| found_node(text, "persisted_symbol") && text.starts_with("_Index: fresh"),
    )?;
    Ok(())
}

#[test]
fn generation_installed_by_another_process_is_picked_up() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn existing_symbol() {}\n")?;
    // Yenileme 2 sn beklerken CLI'ın `update_index`'i yeni generation kurar.
    let mut session = McpSession::start(
        project.path(),
        &[("CCM_INTERNAL_REFRESH_TEST_DELAY_MS", "2000")],
    )?;
    session.call_tool("index_now", json!({ "project_path": project.path() }))?;
    poll_find_nodes(
        &mut session,
        "existing_symbol",
        Duration::from_secs(15),
        |text| text.starts_with("_Index: fresh"),
    )?;
    let generation_before = active_generation_dir(project.path())?;

    fs::write(project.path().join("cli.rs"), "fn cli_symbol() {}\n")?;
    let stats = run_update_index_process(project.path())?;
    assert_eq!(
        stats["files_indexed"], 1,
        "update_index must index the change: {stats}"
    );
    let generation_after = active_generation_dir(project.path())?;
    assert_ne!(
        generation_after, generation_before,
        "update_index must install a new generation"
    );

    let text = poll_find_nodes(
        &mut session,
        "cli_symbol",
        Duration::from_secs(15),
        |text| found_node(text, "cli_symbol") && text.starts_with("_Index: fresh"),
    )?;
    assert!(!text.contains("last refresh failed"), "{text}");

    // Canlı yenileme yeni generation üzerinde sürer ve onun dizinine yazar.
    fs::write(
        project.path().join("after.rs"),
        "fn after_cli_symbol() {}\n",
    )?;
    poll_find_nodes(
        &mut session,
        "after_cli_symbol",
        Duration::from_secs(15),
        |text| found_node(text, "after_cli_symbol") && text.starts_with("_Index: fresh"),
    )?;
    wait_for_persisted_file(project.path(), "./after.rs", Duration::from_secs(10))?;
    assert_eq!(active_generation_dir(project.path())?, generation_after);
    Ok(())
}

#[test]
fn index_with_an_old_schema_is_migrated_before_live_refreshes() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn existing_symbol() {}\n")?;
    run_update_index_process(project.path())?;
    let generation = active_generation_dir(project.path())?;
    let manifest_path = generation.join("ccm_manifest.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path)?)?;
    manifest["schema_version"] = json!(3);
    fs::write(&manifest_path, serde_json::to_vec(&manifest)?)?;

    // Eski şemalı indeks canlı güncellenmez: okumalar onu kullanır, otomatik
    // yenileme `update_index` ile tam yeniden indeksleyip yeni generation kurar.
    let mut session = McpSession::start(project.path(), &[])?;
    fs::write(project.path().join("added.rs"), "fn migrated_symbol() {}\n")?;
    poll_find_nodes(
        &mut session,
        "migrated_symbol",
        Duration::from_secs(30),
        |text| found_node(text, "migrated_symbol") && text.starts_with("_Index: fresh"),
    )?;
    let migrated = active_generation_dir(project.path())?;
    assert_ne!(
        migrated, generation,
        "the migration must be a full re-index into a new generation, not a live stamp"
    );
    let manifest: Value = serde_json::from_slice(&fs::read(migrated.join("ccm_manifest.json"))?)?;
    assert_eq!(manifest["schema_version"], 4);
    Ok(())
}
