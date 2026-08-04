use serde_json::json;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use tempfile::tempdir;

fn send_request(
    stdin: &mut impl Write,
    reader: &mut impl BufRead,
    request: serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    writeln!(stdin, "{}", request)?;
    stdin.flush()?;
    let mut line = String::new();
    reader.read_line(&mut line)?;
    Ok(serde_json::from_str(&line)?)
}

fn tool_text(response: &serde_json::Value) -> &str {
    response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
}

#[test]
fn mcp_index_project_then_get_context() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir()?;
    let project_root = dir.path();

    fs::write(project_root.join("main.rs"), "fn foo() {}\n")?;
    fs::create_dir_all(project_root.join("data"))?;

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
    cmd.env("CCM_DISABLE_EMBEDDER", "1")
        .env("CCM_MCP_DEBUG", "0")
        .env("CCM_ALLOWED_ROOTS", project_root.to_string_lossy().as_ref())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });
    writeln!(stdin, "{}", init_req)?;
    stdin.flush()?;

    let mut line = String::new();
    reader.read_line(&mut line)?;
    assert!(line.contains("\"result\""));
    line.clear();

    let index_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "index_project",
            "arguments": {
                "project_path": project_root.to_string_lossy()
            }
        }
    });
    writeln!(stdin, "{}", index_req)?;
    stdin.flush()?;

    reader.read_line(&mut line)?;
    assert!(line.contains("Project index refreshed successfully"));
    line.clear();

    let context_req = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "get_context",
            "arguments": {
                "file": "main.rs",
                "line": 1,
                "project_path": project_root.to_string_lossy()
            }
        }
    });
    writeln!(stdin, "{}", context_req)?;
    stdin.flush()?;

    reader.read_line(&mut line)?;
    assert!(line.contains("Current:"));

    let _ = child.kill();

    Ok(())
}

#[test]
fn mcp_index_project_reports_when_index_is_current() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir()?;
    let project_root = dir.path();

    fs::write(project_root.join("main.rs"), "fn foo() {}\n")?;
    fs::create_dir_all(project_root.join("data"))?;

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
    cmd.env("CCM_DISABLE_EMBEDDER", "1")
        .env("CCM_MCP_DEBUG", "0")
        .env("CCM_ALLOWED_ROOTS", project_root.to_string_lossy().as_ref())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });
    writeln!(stdin, "{}", init_req)?;
    stdin.flush()?;

    let mut line = String::new();
    reader.read_line(&mut line)?;
    assert!(line.contains("\"result\""));
    line.clear();

    let index_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "index_project",
            "arguments": {
                "project_path": project_root.to_string_lossy()
            }
        }
    });
    writeln!(stdin, "{}", index_req)?;
    stdin.flush()?;

    reader.read_line(&mut line)?;
    assert!(line.contains("Project index refreshed successfully"));
    line.clear();

    let reindex_req = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "index_project",
            "arguments": {
                "project_path": project_root.to_string_lossy()
            }
        }
    });
    writeln!(stdin, "{}", reindex_req)?;
    stdin.flush()?;

    reader.read_line(&mut line)?;
    assert!(line.contains("No changes detected. Existing index is already up to date."));

    let _ = child.kill();

    Ok(())
}

#[test]
fn mcp_find_nodes_returns_node_metadata() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir()?;
    let project_root = dir.path();

    fs::write(
        project_root.join("main.rs"),
        "fn foo() {}\nfn bar() { foo(); }\n",
    )?;
    fs::create_dir_all(project_root.join("data"))?;

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
    cmd.env("CCM_DISABLE_EMBEDDER", "1")
        .env("CCM_MCP_DEBUG", "0")
        .env("CCM_ALLOWED_ROOTS", project_root.to_string_lossy().as_ref())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });
    writeln!(stdin, "{}", init_req)?;
    stdin.flush()?;

    let mut line = String::new();
    reader.read_line(&mut line)?;
    assert!(line.contains("\"result\""));
    line.clear();

    let index_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "index_project",
            "arguments": {
                "project_path": project_root.to_string_lossy()
            }
        }
    });
    writeln!(stdin, "{}", index_req)?;
    stdin.flush()?;

    reader.read_line(&mut line)?;
    assert!(line.contains("Project index"));
    line.clear();

    let find_req = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "find_nodes",
            "arguments": {
                "query": "foo",
                "project_path": project_root.to_string_lossy()
            }
        }
    });
    writeln!(stdin, "{}", find_req)?;
    stdin.flush()?;

    reader.read_line(&mut line)?;
    assert!(line.contains("**Node ID:**"));
    assert!(line.contains("**File:**"));
    assert!(line.contains("foo"));

    let _ = child.kill();

    Ok(())
}

#[test]
fn mcp_rejects_project_outside_allowlist() -> Result<(), Box<dyn std::error::Error>> {
    let allowed_dir = tempdir()?;
    let project_dir = tempdir()?;

    fs::write(project_dir.path().join("main.rs"), "fn foo() {}\n")?;
    fs::create_dir_all(project_dir.path().join("data"))?;

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
    cmd.env("CCM_DISABLE_EMBEDDER", "1")
        .env("CCM_MCP_DEBUG", "0")
        .env(
            "CCM_ALLOWED_ROOTS",
            allowed_dir.path().to_string_lossy().as_ref(),
        )
        .env("CCM_REQUIRE_ALLOWED_ROOTS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });
    writeln!(stdin, "{}", init_req)?;
    stdin.flush()?;

    let mut line = String::new();
    reader.read_line(&mut line)?;
    assert!(line.contains("\"result\""));
    line.clear();

    let index_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "index_project",
            "arguments": {
                "project_path": project_dir.path().to_string_lossy()
            }
        }
    });
    writeln!(stdin, "{}", index_req)?;
    stdin.flush()?;

    reader.read_line(&mut line)?;
    assert!(line.contains("Project path is not allowed"));

    let _ = child.kill();

    Ok(())
}

#[test]
fn mcp_defaults_to_strict_allowlist() -> Result<(), Box<dyn std::error::Error>> {
    // CCM_REQUIRE_ALLOWED_ROOTS hiç verilmezse strict mod varsayılan olmalı.
    let allowed_dir = tempdir()?;
    let project_dir = tempdir()?;

    fs::write(project_dir.path().join("main.rs"), "fn foo() {}\n")?;
    fs::create_dir_all(project_dir.path().join("data"))?;

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
    cmd.env("CCM_DISABLE_EMBEDDER", "1")
        .env("CCM_MCP_DEBUG", "0")
        .env(
            "CCM_ALLOWED_ROOTS",
            allowed_dir.path().to_string_lossy().as_ref(),
        )
        .env_remove("CCM_REQUIRE_ALLOWED_ROOTS")
        .env_remove("CCM_MCP_REQUIRE_ALLOWED_ROOTS")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());

    let initialized = send_request(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    )?;
    assert!(initialized.get("result").is_some());

    let denied = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"index_project","arguments":{
                "project_path":project_dir.path().to_string_lossy()
            }}
        }),
    )?;
    assert!(tool_text(&denied).contains("not allowed") || denied.get("error").is_some());

    let _ = child.kill();
    Ok(())
}

#[test]
fn mcp_non_strict_empty_allowlist_stays_within_default_root(
) -> Result<(), Box<dyn std::error::Error>> {
    // Strict mod kapalı ve allowlist boşken bile keyfi dizinler indekslenemez;
    // yalnızca başlangıçta seçilen default proje kökü kabul edilir.
    let default_root = tempdir()?;
    let outside_dir = tempdir()?;

    fs::write(outside_dir.path().join("main.rs"), "fn foo() {}\n")?;
    fs::create_dir_all(outside_dir.path().join("data"))?;

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
    cmd.env("CCM_DISABLE_EMBEDDER", "1")
        .env("CCM_MCP_DEBUG", "0")
        .env("CCM_REQUIRE_ALLOWED_ROOTS", "0")
        .env_remove("CCM_ALLOWED_ROOTS")
        .env_remove("CCM_PROJECT_ROOT")
        .current_dir(default_root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());

    let initialized = send_request(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    )?;
    assert!(initialized.get("result").is_some());

    let denied = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"index_project","arguments":{
                "project_path":outside_dir.path().to_string_lossy()
            }}
        }),
    )?;
    assert!(tool_text(&denied).contains("not allowed") || denied.get("error").is_some());

    let _ = child.kill();
    Ok(())
}

#[test]
fn mcp_resolves_class_import_constructor_context_and_impact(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir()?;
    let project_root = dir.path();

    fs::write(
        project_root.join("detector.py"),
        "class YoloDetector:\n    def detect(self):\n        return []\n",
    )?;
    fs::write(
        project_root.join("camera.py"),
        "from detector import YoloDetector\n\n\
         def open_camera(detector: YoloDetector):\n    return YoloDetector()\n\n\
         def boot():\n    return open_camera(YoloDetector())\n",
    )?;

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
    cmd.env("CCM_DISABLE_EMBEDDER", "1")
        .env("CCM_MCP_DEBUG", "0")
        .env("CCM_PROJECT_ROOT", project_root)
        .env("CCM_ALLOWED_ROOTS", project_root)
        .env("CCM_REQUIRE_ALLOWED_ROOTS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());

    let initialized = send_request(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    )?;
    assert!(initialized.get("result").is_some());

    let indexed = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"index_project","arguments":{"project_path":project_root}}
        }),
    )?;
    assert!(tool_text(&indexed).contains("Project index refreshed successfully"));

    let found = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"find_nodes","arguments":{
                "query":"YoloDetector","project_path":project_root
            }}
        }),
    )?;
    let found_text = tool_text(&found);
    let node_id = found_text
        .lines()
        .find_map(|line| line.strip_prefix("**Node ID:** "))
        .expect("YoloDetector stable node ID");
    assert!(node_id.contains(":symbol:"));

    let usages = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":4,"method":"tools/call",
            "params":{"name":"find_usages","arguments":{
                "node_id":node_id,"project_path":project_root
            }}
        }),
    )?;
    let usages_text = tool_text(&usages);
    assert!(usages_text.contains("./camera.py"));
    assert!(usages_text.contains("open_camera"));

    let context = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":5,"method":"tools/call",
            "params":{"name":"get_context","arguments":{
                "file":"camera.py","line":4,"project_path":project_root
            }}
        }),
    )?;
    let context_text = tool_text(&context);
    assert!(context_text.contains("Current: open_camera"));
    assert!(context_text.contains("def open_camera"));

    let graph = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":6,"method":"tools/call",
            "params":{"name":"read_graph","arguments":{
                "node_id":node_id,"project_path":project_root
            }}
        }),
    )?;
    assert!(tool_text(&graph).contains("Node Details: YoloDetector"));

    let impact = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":7,"method":"tools/call",
            "params":{"name":"impact_of_change","arguments":{
                "file":"detector.py","project_path":project_root
            }}
        }),
    )?;
    let impact_text = tool_text(&impact);
    assert!(impact_text.contains("./camera.py"));
    assert!(impact_text.contains("open_camera"));

    let _ = child.kill();
    Ok(())
}
