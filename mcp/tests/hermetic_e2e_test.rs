//! Sentetik fixture (repo_a) üzerinde 9 MCP aracının tamamını uçtan uca test
//! eder. Guardian projesi gibi dış bağımlılık yoktur; CI'da çalışır.
//!
//! Run with:
//!   cargo test -p ccm-mcp --test hermetic_e2e_test

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn send(stdin: &mut impl Write, reader: &mut BufReader<impl std::io::Read>, req: Value) -> Value {
    writeln!(stdin, "{}", req).expect("write to MCP stdin");
    stdin.flush().expect("flush to MCP stdin");
    let mut line = String::new();
    reader.read_line(&mut line).expect("read from MCP stdout");
    serde_json::from_str(line.trim())
        .unwrap_or_else(|e| panic!("MCP response is not valid JSON: {}\nRaw: {}", e, line))
}

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

fn source_repos() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("eval/fixtures/repos")
}

#[test]
fn hermetic_mcp_all_tools_e2e_on_synthetic_repo() {
    // 1. Commit'li sentetik fixture repo'yu doğrudan kullan (hermetic: dış
    // bağımlılık yok) ve structural index kur.
    let repo_a = source_repos().join("repo_a");
    assert!(
        repo_a.join("src/pricing.rs").is_file(),
        "fixture src/pricing.rs missing at {}",
        repo_a.display()
    );

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(async {
        std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
        // index_directory her zaman full re-index yapar; update_index mevcut
        // manifest varsa no-op dönebilir (fixture data/ diskte kalabilir).
        let stats = ccm_core::index_directory(
            &repo_a.to_string_lossy(),
            Some(&repo_a.join("data/ccm_db").to_string_lossy()),
        )
        .await
        .expect("index repo_a");
        assert!(
            stats.nodes_created > 0,
            "repo_a nodes_created={} files_indexed={} files_failed={} reasons={:?}",
            stats.nodes_created,
            stats.files_indexed,
            stats.files_failed,
            stats.reason_counts
        );
    });

    // 2. MCP server spawn (strict allowlist repo_a'ya işaret eder).
    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"))
        .env("CCM_DISABLE_EMBEDDER", "1")
        .env("CCM_MCP_DEBUG", "0")
        .env("CCM_ALLOWED_ROOTS", &repo_a)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn ccm-mcp");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    // 3. initialize + tools/list: 9 araç.
    let resp = send(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}),
    );
    assert_no_error(&resp, "0 initialize");
    assert!(resp["result"]["protocolVersion"].as_str().is_some());

    let resp = send(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
    );
    assert_no_error(&resp, "1 tools/list");
    let tools: Vec<String> = resp["result"]["tools"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|t| t["name"].as_str().map(str::to_string))
        .collect();
    for name in [
        "search_code",
        "get_context",
        "find_nodes",
        "read_graph",
        "index_project",
        "find_usages",
        "trace_call_chain",
        "impact_of_change",
        "diff_context",
    ] {
        assert!(tools.iter().any(|t| t == name), "missing tool {}", name);
    }

    // 4. index_project (no-op beklenir).
    let resp = send(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"index_project","arguments":{"project_path": repo_a.to_string_lossy()}}
        }),
    );
    assert_no_error(&resp, "2 index_project");
    let text = text_of(&resp);
    assert!(
        text.contains("Project index refreshed") || text.contains("already up to date"),
        "unexpected index_project response: {}",
        text
    );

    // 5. search_code.
    let resp = send(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"search_code","arguments":{
                "query":"compute_tax","project_path": repo_a.to_string_lossy()
            }}
        }),
    );
    assert_no_error(&resp, "3 search_code");
    assert!(!text_of(&resp).is_empty());

    // 6. find_nodes → compute_tax node.
    let resp = send(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":4,"method":"tools/call",
            "params":{"name":"find_nodes","arguments":{
                "query":"compute_tax","project_path": repo_a.to_string_lossy()
            }}
        }),
    );
    assert_no_error(&resp, "4 find_nodes");
    let find_text = text_of(&resp);
    assert!(find_text.contains("compute_tax"), "find_nodes: {}", find_text);
    // Metadata-first çıktı: node_id mevcut, gövde yok.
    assert!(!find_text.contains("fn compute_tax"), "varsayılan body içermemeli");

    // 7. read_graph: find_nodes çıktısından ilk node id'yi çöz.
    let node_id = find_text
        .lines()
        .find_map(|line| {
            line.strip_prefix("**Node ID:** ")
                .map(|id| id.trim().to_string())
        })
        .expect("find_nodes must return a Node ID");
    let resp = send(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":5,"method":"tools/call",
            "params":{"name":"read_graph","arguments":{
                "node_id": node_id, "project_path": repo_a.to_string_lossy()
            }}
        }),
    );
    assert_no_error(&resp, "5 read_graph");
    assert!(text_of(&resp).contains("compute_tax"));

    // 8. get_context: tax.rs'de compute_tax satırı.
    let resp = send(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":6,"method":"tools/call",
            "params":{"name":"get_context","arguments":{
                "file":"src/tax.rs","line":3,"project_path": repo_a.to_string_lossy(),
                "include_body":true
            }}
        }),
    );
    assert_no_error(&resp, "6 get_context");
    assert!(text_of(&resp).contains("compute_tax"));

    // 9. find_usages: compute_tax'ı kim çağırıyor → pricing.rs.
    let resp = send(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":7,"method":"tools/call",
            "params":{"name":"find_usages","arguments":{
                "node_id": node_id, "project_path": repo_a.to_string_lossy()
            }}
        }),
    );
    assert_no_error(&resp, "7 find_usages");
    let usages = text_of(&resp);
    assert!(
        usages.contains("pricing.rs") || usages.contains("compute_total"),
        "find_usages should surface pricing.rs caller: {}",
        usages
    );

    // 10. trace_call_chain: compute_total → compute_tax.
    let total_id = {
        let resp = send(
            &mut stdin,
            &mut reader,
            json!({
                "jsonrpc":"2.0","id":8,"method":"tools/call",
                "params":{"name":"find_nodes","arguments":{
                    "query":"compute_total","project_path": repo_a.to_string_lossy()
                }}
            }),
        );
        assert_no_error(&resp, "8 find_nodes compute_total");
        text_of(&resp)
            .lines()
            .find_map(|line| {
                line.strip_prefix("**Node ID:** ")
                    .map(|id| id.trim().to_string())
            })
            .expect("compute_total node id")
    };
    let resp = send(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":9,"method":"tools/call",
            "params":{"name":"trace_call_chain","arguments":{
                "from_id": total_id, "to_id": node_id,
                "max_depth": 4, "project_path": repo_a.to_string_lossy()
            }}
        }),
    );
    assert_no_error(&resp, "9 trace_call_chain");

    // 11. impact_of_change: pricing.rs.
    let resp = send(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":10,"method":"tools/call",
            "params":{"name":"impact_of_change","arguments":{
                "file":"src/pricing.rs","project_path": repo_a.to_string_lossy()
            }}
        }),
    );
    assert_no_error(&resp, "10 impact_of_change");

    // 12. diff_context: son N günde değişen kod (git repo'su olmayabilir → boş OK).
    let resp = send(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":11,"method":"tools/call",
            "params":{"name":"diff_context","arguments":{
                "project_path": repo_a.to_string_lossy(), "days": 7
            }}
        }),
    );
    assert_no_error(&resp, "11 diff_context");

    drop(stdin);
    let _ = child.wait();
}
