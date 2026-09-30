//! MCP protokol sözleşmesi: araç açıklamaları ve yetenek bildirimi gerçek
//! `ccm-mcp` süreciyle doğrulanır.

mod common;

use common::{McpSession, TestResult};
use serde_json::json;
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
