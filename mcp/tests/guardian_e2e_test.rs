/// Guardian project — gerçek proje üzerinde tüm 9 MCP aracını uçtan uca test eder.
///
/// Run with:
///   cargo test --package ccm-mcp --test guardian_e2e_test -- --nocapture
///   (can take ~30 s — the guardian project is large)
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn guardian_root() -> std::path::PathBuf {
    std::env::var_os("CCM_GUARDIAN_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/Users/dogan/Desktop/guardian"))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Sends one newline-delimited JSON-RPC request and reads one response line.
fn send(stdin: &mut impl Write, reader: &mut BufReader<impl std::io::Read>, req: Value) -> Value {
    writeln!(stdin, "{}", req).expect("write to MCP stdin");
    stdin.flush().expect("flush to MCP stdin");
    let mut line = String::new();
    reader.read_line(&mut line).expect("read from MCP stdout");
    serde_json::from_str(line.trim())
        .unwrap_or_else(|e| panic!("MCP response is not valid JSON: {}\nRaw: {}", e, line))
}

/// Extracts the readable text from a successful tool/call response.
fn text_of(resp: &Value) -> String {
    resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("[no text content]")
        .to_string()
}

fn assert_no_error(resp: &Value, step: &str) {
    assert!(
        resp.get("error").is_none(),
        "STEP {} returned an error: {}",
        step,
        resp
    );
}

// ---------------------------------------------------------------------------
// Test
// ---------------------------------------------------------------------------

/// Run locally against the real guardian project:
///   cargo test --package ccm-mcp --test guardian_e2e_test -- --nocapture --ignored
#[test]
#[ignore = "requires local guardian project at /Users/dogan/Desktop/guardian"]
fn guardian_mcp_all_tools_e2e() {
    // Guard: project must exist
    assert!(
        guardian_root().exists(),
        "Guardian project not found at {}",
        guardian_root().display()
    );

    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║   Guardian MCP End-to-End Test  (real project)          ║");
    println!("╚══════════════════════════════════════════════════════════╝");
    println!("  Project: {}", guardian_root().display());

    // -----------------------------------------------------------------------
    // Spawn MCP server
    // -----------------------------------------------------------------------
    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"))
        .env("CCM_DISABLE_EMBEDDER", "1") // no OpenAI key needed
        .env("CCM_MCP_DEBUG", "0")
        .env("CCM_ALLOWED_ROOTS", guardian_root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null()) // suppress tracing noise in test output
        .spawn()
        .expect("failed to spawn ccm-mcp");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    // =======================================================================
    // STEP 0 — initialize (MCP protocol handshake)
    // =======================================================================
    println!("\n── STEP 0: initialize ──────────────────────────────────────");
    let resp = send(
        &mut stdin,
        &mut reader,
        json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {} }),
    );
    assert_no_error(&resp, "0 initialize");
    assert!(
        resp["result"]["protocolVersion"].as_str().is_some(),
        "protocolVersion missing"
    );
    assert!(
        resp["result"]["capabilities"]["tools"].is_object(),
        "tools capability missing"
    );
    println!(
        "  ✓ protocolVersion = {}",
        resp["result"]["protocolVersion"]
    );
    println!("  ✓ tools capability present");

    // =======================================================================
    // STEP 1 — tools/list: all 9 tools must be registered
    // =======================================================================
    println!("\n── STEP 1: tools/list ──────────────────────────────────────");
    let resp = send(
        &mut stdin,
        &mut reader,
        json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {} }),
    );
    assert_no_error(&resp, "1 tools/list");

    let tools: Vec<String> = resp["result"]["tools"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|t| t["name"].as_str().map(str::to_string))
        .collect();

    let required_tools = [
        "map",
        "explain",
        "search_code",
        "find_nodes",
        "index_project",
        "find_usages",
        "trace_call_chain",
        "impact_of_change",
        "diff_context",
    ];
    for name in &required_tools {
        assert!(
            tools.iter().any(|t| t == name),
            "tool '{}' not in tools/list — got: {:?}",
            name,
            tools
        );
    }
    println!("  ✓ {} tools registered: {:?}", tools.len(), tools);

    // =======================================================================
    // STEP 2 — index_project: index the real guardian codebase
    // =======================================================================
    println!("\n── STEP 2: index_project (guardian — first pass) ──────────");
    let resp = send(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc": "2.0", "id": 2,
            "method": "tools/call",
            "params": {
                "name": "index_project",
                "arguments": { "project_path": guardian_root() }
            }
        }),
    );
    assert_no_error(&resp, "2 index_project");
    let text = text_of(&resp);
    assert!(
        text.contains("Project index refreshed") || text.contains("already up to date"),
        "unexpected index_project response: {}",
        text
    );
    println!("  ✓ {}", text.lines().next().unwrap_or(""));

    // =======================================================================
    // STEP 3 — search_code: search for AI audit functionality
    // =======================================================================
    println!("\n── STEP 3: search_code (\"AI audit critique generation\") ──");
    let resp = send(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc": "2.0", "id": 3,
            "method": "tools/call",
            "params": {
                "name": "search_code",
                "arguments": {
                    "query": "AI audit critique generation severity",
                    "project_path": guardian_root()
                }
            }
        }),
    );
    assert_no_error(&resp, "3 search_code");
    let text = text_of(&resp);
    assert!(!text.is_empty(), "search_code must return content");
    let lower = text.to_lowercase();
    assert!(
        lower.contains("critique")
            || lower.contains("audit")
            || lower.contains("ai")
            || lower.contains("severity")
            || lower.contains("no results"),
        "search_code must return a valid response, got: {}",
        &text[..text.len().min(400)]
    );
    println!(
        "  ✓ {} chars — {}",
        text.len(),
        text.lines().next().unwrap_or("").trim()
    );

    // =======================================================================
    // STEP 4 — map: most-used files under src-tauri/src
    // =======================================================================
    println!("\n── STEP 4: map (src-tauri/src) ─────");
    let resp = send(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc": "2.0", "id": 4,
            "method": "tools/call",
            "params": {
                "name": "map",
                "arguments": {
                    "path": "src-tauri/src",
                    "project_path": guardian_root()
                }
            }
        }),
    );
    assert_no_error(&resp, "4 map");
    let text = text_of(&resp);
    assert!(
        text.contains("files,"),
        "map must start with its header: {text}"
    );
    println!(
        "  ✓ {} chars — {}",
        text.len(),
        text.lines().next().unwrap_or("").trim()
    );

    // =======================================================================
    // STEP 5 — find_nodes: locate nodes related to AiClient
    // =======================================================================
    println!("\n── STEP 5: find_nodes (\"AiClient\") ────────────────────────");
    let resp = send(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc": "2.0", "id": 5,
            "method": "tools/call",
            "params": {
                "name": "find_nodes",
                "arguments": {
                    "query": "AiClient",
                    "project_path": guardian_root()
                }
            }
        }),
    );
    assert_no_error(&resp, "5 find_nodes");
    let text = text_of(&resp);
    assert!(!text.is_empty(), "find_nodes must return content");
    let lower = text.to_lowercase();
    assert!(
        lower.contains("aiclient") || lower.contains("ai_client"),
        "find_nodes must locate AiClient, got: {}",
        &text[..text.len().min(400)]
    );
    println!(
        "  ✓ {} chars — {}",
        text.len(),
        text.lines().next().unwrap_or("").trim()
    );

    // İlk kompakt satırın `yol:başlangıç` konumu sonraki adımların hedefidir.
    let node_id = text
        .lines()
        .find_map(|line| {
            let location = line.strip_prefix("- ")?.split(" · ").nth(1)?;
            let (path, range) = location.rsplit_once(':')?;
            Some(format!("{path}:{}", range.split('-').next()?))
        })
        .expect("find_nodes must return a path:line handle");
    println!("  ✓ using target: {}", node_id);

    // =======================================================================
    // STEP 6 — explain: inspect the symbol in one call
    // =======================================================================
    println!(
        "\n── STEP 6: explain ({}) ─",
        &node_id[..node_id.len().min(40)]
    );
    let resp = send(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc": "2.0", "id": 6,
            "method": "tools/call",
            "params": {
                "name": "explain",
                "arguments": {
                    "target": &node_id,
                    "project_path": guardian_root()
                }
            }
        }),
    );
    assert_no_error(&resp, "6 explain");
    let text = text_of(&resp);
    assert!(!text.is_empty(), "explain must return content");
    println!(
        "  ✓ {} chars — {}",
        text.len(),
        text.lines().next().unwrap_or("").trim()
    );

    // =======================================================================
    // STEP 7 — find_usages: who uses that node?
    // =======================================================================
    println!("\n── STEP 7: find_usages ─────────────────────────────────────");
    let resp = send(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc": "2.0", "id": 7,
            "method": "tools/call",
            "params": {
                "name": "find_usages",
                "arguments": {
                    "node_id": &node_id,
                    "project_path": guardian_root()
                }
            }
        }),
    );
    assert_no_error(&resp, "7 find_usages");
    let text = text_of(&resp);
    assert!(!text.is_empty(), "find_usages must return content");
    println!(
        "  ✓ {} chars — {}",
        text.len(),
        text.lines().next().unwrap_or("").trim()
    );

    // =======================================================================
    // STEP 8 — trace_call_chain: triage → context
    // =======================================================================
    println!("\n── STEP 8: trace_call_chain (TriageResult → ProjectContext) ─");
    let resp = send(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc": "2.0", "id": 8,
            "method": "tools/call",
            "params": {
                "name": "trace_call_chain",
                "arguments": {
                    "from_id": "./src-tauri/src/triage.rs::TriageResult",
                    "to_id":   "./src-tauri/src/context.rs::ProjectContext",
                    "project_path": guardian_root()
                }
            }
        }),
    );
    assert_no_error(&resp, "8 trace_call_chain");
    let text = text_of(&resp);
    assert!(!text.is_empty(), "trace_call_chain must return content");
    println!("  ✓ {}", text.lines().next().unwrap_or("").trim());

    // =======================================================================
    // STEP 9 — impact_of_change: who is affected when ai_client.rs changes?
    // =======================================================================
    println!("\n── STEP 9: impact_of_change (src-tauri/src/ai_client.rs) ──");
    let resp = send(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc": "2.0", "id": 9,
            "method": "tools/call",
            "params": {
                "name": "impact_of_change",
                "arguments": {
                    "file": "src-tauri/src/ai_client.rs",
                    "project_path": guardian_root()
                }
            }
        }),
    );
    assert_no_error(&resp, "9 impact_of_change");
    let text = text_of(&resp);
    assert!(!text.is_empty(), "impact_of_change must return content");
    let lower = text.to_lowercase();
    assert!(
        lower.contains("impact") || lower.contains("dependent") || lower.contains("no dependent"),
        "impact_of_change must describe dependents, got: {}",
        &text[..text.len().min(300)]
    );
    println!(
        "  ✓ {} chars — {}",
        text.len(),
        text.lines().next().unwrap_or("").trim()
    );

    // =======================================================================
    // STEP 10 — diff_context: surfaces recent git commits in guardian
    // =======================================================================
    println!("\n── STEP 10: diff_context (days=30) ─────────────────────────");
    let resp = send(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc": "2.0", "id": 10,
            "method": "tools/call",
            "params": {
                "name": "diff_context",
                "arguments": {
                    "project_path": guardian_root(),
                    "days": 30
                }
            }
        }),
    );
    assert_no_error(&resp, "10 diff_context");
    let text = text_of(&resp);
    assert!(!text.is_empty(), "diff_context must return content");
    let lower = text.to_lowercase();
    assert!(
        lower.contains("recently changed")
            || lower.contains("no recent")
            || lower.contains("last")
            || lower.contains("days"),
        "diff_context must describe recent changes, got: {}",
        &text[..text.len().min(300)]
    );
    println!(
        "  ✓ {} chars — {}",
        text.len(),
        text.lines().next().unwrap_or("").trim()
    );

    // =======================================================================
    // STEP 11 — index_project (2nd call): must detect no changes (idempotent)
    // =======================================================================
    println!("\n── STEP 11: index_project (idempotency check) ──────────────");
    let resp = send(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc": "2.0", "id": 11,
            "method": "tools/call",
            "params": {
                "name": "index_project",
                "arguments": { "project_path": guardian_root() }
            }
        }),
    );
    assert_no_error(&resp, "11 index_project 2nd");
    let text = text_of(&resp);
    assert!(
        text.contains("No changes detected") || text.contains("already up to date"),
        "2nd index_project must be idempotent, got: {}",
        text
    );
    println!("  ✓ {}", text.lines().next().unwrap_or("").trim());

    // Done
    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║  ✅  All 9 tools PASSED — guardian E2E test COMPLETE     ║");
    println!("╚══════════════════════════════════════════════════════════╝\n");

    let _ = child.kill();
    let _ = child.wait();
}
