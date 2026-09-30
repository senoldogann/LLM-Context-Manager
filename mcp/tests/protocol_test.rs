//! MCP protokol sözleşmesi: eşzamanlı istekler, iptal, araç açıklamaları ve
//! yetenek bildirimi gerçek `ccm-mcp` süreciyle doğrulanır.

mod common;

use common::{
    found_node, poll_find_nodes, run_update_index_process, tool_text, McpSession, TestResult,
    READ_TIMEOUT,
};
use serde_json::json;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};
use tempfile::tempdir;

/// Salt okunur araçlar ve beklenen davranış ipuçları.
const READ_TOOLS: [&str; 8] = [
    "get_context",
    "search_code",
    "find_nodes",
    "read_graph",
    "find_usages",
    "trace_call_chain",
    "impact_of_change",
    "diff_context",
];

#[test]
fn slow_tool_call_does_not_delay_concurrent_requests() -> TestResult<()> {
    let project = tempdir()?;
    fs::write(
        project.path().join("main.rs"),
        "fn concurrent_symbol() {}\n",
    )?;
    // İndeks ayrı bir süreçte kurulur; sunucunun worker'ı her çağrıda 5 sn gecikir.
    run_update_index_process(project.path())?;
    let mut session = McpSession::start(
        project.path(),
        &[("CCM_INTERNAL_INDEX_TEST_DELAY_MS", "5000")],
    )?;
    // Başlangıç karşılaştırması bitmeden okumalar tazelik bütçesini bekleyebilir.
    poll_find_nodes(
        &mut session,
        "concurrent_symbol",
        Duration::from_secs(15),
        |text| text.starts_with("_Index: fresh"),
    )?;

    let slow = session.send_request(
        "tools/call",
        json!({"name": "index_now", "arguments": {"project_path": project.path()}}),
    )?;
    let ping_started = Instant::now();
    let ping = session.send_request("ping", json!({}))?;
    let ping_response = session.wait_for_response(ping, READ_TIMEOUT)?;
    let ping_elapsed = ping_started.elapsed();
    let find_started = Instant::now();
    let find = session.send_request(
        "tools/call",
        json!({"name": "find_nodes", "arguments": {"query": "concurrent_symbol"}}),
    )?;
    let find_response = session.wait_for_response(find, READ_TIMEOUT)?;
    let find_elapsed = find_started.elapsed();

    assert!(
        !session.has_received(slow),
        "index_now (5 s worker delay) must still be running"
    );
    assert_eq!(ping_response["result"], json!({}), "{ping_response}");
    assert!(
        ping_elapsed < Duration::from_secs(1),
        "ping waited {ping_elapsed:?} behind the slow tool call"
    );
    let find_text = tool_text("find_nodes", &find_response)?;
    assert!(found_node(&find_text, "concurrent_symbol"), "{find_text}");
    assert!(
        find_elapsed < Duration::from_secs(1),
        "find_nodes waited {find_elapsed:?} behind the slow tool call"
    );

    let slow_response = session.wait_for_response(slow, READ_TIMEOUT)?;
    let slow_text = tool_text("index_now", &slow_response)?;
    assert!(
        slow_text.contains("already up to date")
            || slow_text.contains("Project index refreshed successfully"),
        "{slow_text}"
    );
    Ok(())
}

#[test]
fn cancelled_tool_call_gets_no_response_and_the_server_keeps_serving() -> TestResult<()> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn cancel_symbol() {}\n")?;
    let mut session = McpSession::start(
        project.path(),
        &[
            ("CCM_INTERNAL_INDEX_TEST_DELAY_MS", "4000"),
            ("CCM_AUTO_REFRESH", "0"),
        ],
    )?;
    let index_now = json!({"name": "index_now", "arguments": {"project_path": project.path()}});

    let cancelled = session.send_request("tools/call", index_now.clone())?;
    session.notify(
        "notifications/cancelled",
        json!({"requestId": cancelled, "reason": "test cancels the slow call"}),
    )?;
    // Bilinmeyen kimlik: iptal edilecek bir şey yoktur, sunucu hizmete devam eder.
    session.notify("notifications/cancelled", json!({"requestId": 9_999}))?;
    let ping = session.request("ping", json!({}))?;
    assert_eq!(ping["result"], json!({}), "{ping}");

    // İptal yalnızca beklemeyi durdurur: indeksleme işi sürer ve aynı projedeki
    // sonraki index_now o işe katılıp sonucunu alır.
    let retry = session.send_request("tools/call", index_now)?;
    let retry_response = session.wait_for_response(retry, READ_TIMEOUT)?;
    let retry_text = tool_text("index_now", &retry_response)?;
    assert!(
        retry_text.contains("Project index refreshed successfully"),
        "{retry_text}"
    );
    assert_no_late_response(&mut session, cancelled)?;
    Ok(())
}

#[test]
fn cancelled_index_now_finishes_indexing_and_the_next_call_joins_it() -> TestResult<()> {
    const FILES: usize = 800;
    let project = tempdir()?;
    fs::create_dir_all(project.path().join("src"))?;
    for index in 0..FILES {
        fs::write(
            project.path().join(format!("src/module_{index}.rs")),
            format!("pub fn indexed_function_{index}() -> usize {{ {index} }}\n"),
        )?;
    }
    let mut session = McpSession::start(project.path(), &[("CCM_AUTO_REFRESH", "0")])?;
    let index_now = json!({"name": "index_now", "arguments": {"project_path": project.path()}});

    // İstemci (ör. SDK zaman aşımı) worker dosyaları işlerken iptal eder: tam
    // indeksin staging dizini kurulmuş, generation henüz etkinleşmemiştir.
    let cancelled = session.send_request("tools/call", index_now.clone())?;
    let generations = project.path().join("data/.ccm-generations");
    let deadline = Instant::now() + Duration::from_secs(60);
    while staging_dirs(&generations)?.is_empty() {
        assert!(
            Instant::now() < deadline,
            "the index worker never created its staging generation"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    session.notify(
        "notifications/cancelled",
        json!({"requestId": cancelled, "reason": "client timeout"}),
    )?;

    // Sonraki çağrı süren işe katılır; kilit beklemesi (60 sn) yaşanmaz.
    let started = Instant::now();
    let joined = session.call_tool("index_now", index_now["arguments"].clone())?;
    let waited = started.elapsed();
    assert!(
        joined.contains("Project index refreshed successfully"),
        "{joined}"
    );
    assert!(
        joined.contains(&format!("Files Indexed: {FILES}")),
        "the index must be complete: {joined}"
    );
    assert!(
        waited < Duration::from_secs(45),
        "the follow-up index_now waited {waited:?}"
    );
    assert_no_late_response(&mut session, cancelled)?;

    assert_activation_lock_is_free(&project.path().join("data"))?;
    assert_eq!(
        staging_dirs(&generations)?,
        Vec::<String>::new(),
        "staging copies were left behind"
    );
    let last_symbol = format!("indexed_function_{}", FILES - 1);
    let found = session.call_tool("find_nodes", json!({ "query": last_symbol }))?;
    assert!(found_node(&found, &last_symbol), "{found}");
    Ok(())
}

/// Etkinleştirme kilidini hiçbir süreç tutmuyor: kilit dosyası ya yok ya da hemen
/// kilitlenebiliyor. Kilit dosyası bırakılırken silinmediği için varlığı kilidin
/// tutulduğunu göstermez.
fn assert_activation_lock_is_free(data_dir: &Path) -> TestResult<()> {
    let lock_path = data_dir.join(".ccm-activation.flock");
    let file = match fs::OpenOptions::new().write(true).open(&lock_path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    match file.try_lock() {
        Ok(()) => Ok(()),
        Err(fs::TryLockError::WouldBlock) => Err(format!(
            "the activation lock '{}' is still held after the index job finished",
            lock_path.display()
        )
        .into()),
        Err(fs::TryLockError::Error(error)) => Err(error.into()),
    }
}

/// Generation dizinindeki bitmemiş (`.staging`) kopyalar.
fn staging_dirs(generations: &Path) -> TestResult<Vec<String>> {
    if !generations.exists() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(generations)? {
        let name = entry?.file_name().to_string_lossy().to_string();
        if name.ends_with(".staging") {
            names.push(name);
        }
    }
    Ok(names)
}

/// İptal edilen isteğe hiç yanıt gelmediğini doğrular. İptal edilmemiş bir
/// bekleyici iş bitince diğeriyle aynı anda uyanırdı; kısa bekleme ve ardından
/// gelen ping, onun yanıtının da okunmuş olmasını sağlar.
fn assert_no_late_response(session: &mut McpSession, cancelled: u64) -> TestResult<()> {
    std::thread::sleep(Duration::from_millis(500));
    let ping = session.request("ping", json!({}))?;
    assert_eq!(ping["result"], json!({}), "{ping}");
    assert!(
        !session.has_received(cancelled),
        "a cancelled request must not be answered"
    );
    Ok(())
}

#[test]
fn tools_list_exposes_titles_and_annotations() -> TestResult<()> {
    let project = tempdir()?;
    let mut session = McpSession::start(project.path(), &[])?;

    let response = session.request("tools/list", json!({}))?;
    let tools = response["result"]["tools"]
        .as_array()
        .ok_or_else(|| format!("tools/list has no tools array: {response}"))?;
    let mut names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    names.sort_unstable();
    let mut expected_names: Vec<&str> = READ_TOOLS.to_vec();
    expected_names.extend(["index_project", "index_now"]);
    expected_names.sort_unstable();
    assert_eq!(names, expected_names);

    for tool in tools {
        let name = tool["name"].as_str().unwrap_or_default();
        let title = tool["title"].as_str().unwrap_or_default();
        assert!(!title.trim().is_empty(), "{name} has no title: {tool}");
        let expected = if READ_TOOLS.contains(&name) {
            json!({
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            })
        } else {
            json!({
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            })
        };
        assert_eq!(tool["annotations"], expected, "{name}: {tool}");
    }
    Ok(())
}

#[test]
fn initialize_advertises_only_implemented_capabilities() -> TestResult<()> {
    let project = tempdir()?;
    let mut session = McpSession::spawn(project.path(), &[])?;

    let initialized = session.request(
        "initialize",
        json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "protocol-test", "version": "0"}
        }),
    )?;
    assert_eq!(
        initialized["result"]["protocolVersion"], "2025-06-18",
        "{initialized}"
    );
    assert_eq!(
        initialized["result"]["capabilities"],
        json!({
            "tools": {"listChanged": false},
            "resources": {"subscribe": false, "listChanged": false}
        }),
        "{initialized}"
    );

    // Uygulanmayan yöntemler JSON-RPC hatası olarak kalır.
    let unknown = session.request("prompts/list", json!({}))?;
    assert_eq!(unknown["error"]["code"], -32601, "{unknown}");
    Ok(())
}
