use serde_json::json;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
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
fn mcp_large_index_returns_before_client_timeout_and_supports_polling(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir()?;
    let project_root = dir.path();
    fs::create_dir_all(project_root.join("src"))?;
    for index in 0..300 {
        fs::write(
            project_root.join("src").join(format!("module_{index}.rs")),
            format!("pub fn function_{index}() -> usize {{ {index} }}\n"),
        )?;
    }

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
    cmd.env("CCM_DISABLE_EMBEDDER", "1")
        .env("CCM_MCP_DEBUG", "0")
        .env("CCM_INDEX_RESPONSE_TIMEOUT_MS", "1")
        .env("CCM_PROJECT_ROOT", project_root.to_string_lossy().as_ref())
        .env("CCM_ALLOWED_ROOTS", project_root.to_string_lossy().as_ref())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let initialized = send_request(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}),
    )?;
    assert!(initialized.get("result").is_some());

    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "index_project",
            "arguments": { "project_path": project_root }
        }
    });

    let started_at = Instant::now();
    let started = send_request(&mut stdin, &mut reader, request.clone())?;
    assert!(
        started_at.elapsed() < Duration::from_secs(5),
        "background index acknowledgement exceeded five seconds"
    );
    assert!(tool_text(&started).contains("started in the background"));

    let in_progress = send_request(&mut stdin, &mut reader, request.clone())?;
    assert!(tool_text(&in_progress).contains("still in progress"));

    let retrieval = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0",
            "id":2,
            "method":"tools/call",
            "params":{
                "name":"get_context",
                "arguments":{
                    "file":"src/module_0.rs",
                    "line":1
                }
            }
        }),
    )?;
    assert_eq!(retrieval["error"]["code"], -32603);
    assert!(retrieval["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("indexing is in progress"));

    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let response = send_request(&mut stdin, &mut reader, request.clone())?;
        let text = tool_text(&response);
        if text.contains("Project index refreshed successfully") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "background index did not finish before deadline: {text}"
        );
    }

    let _ = child.kill();
    Ok(())
}

#[test]
fn mcp_index_worker_timeout_releases_the_job_for_retry() -> Result<(), Box<dyn std::error::Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn delayed() {}\n")?;
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
    cmd.env("CCM_DISABLE_EMBEDDER", "1")
        .env("CCM_MCP_DEBUG", "0")
        .env("CCM_PROJECT_ROOT", project.path())
        .env("CCM_ALLOWED_ROOTS", project.path())
        .env("CCM_INDEX_EXECUTION_TIMEOUT_MS", "50")
        .env("CCM_INTERNAL_INDEX_TEST_DELAY_MS", "500")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let initialized = send_request(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}),
    )?;
    assert!(initialized.get("result").is_some());
    let request = json!({
        "jsonrpc":"2.0","id":1,"method":"tools/call",
        "params":{"name":"index_project","arguments":{"project_path":project.path()}}
    });

    let started = Instant::now();
    let timed_out = send_request(&mut stdin, &mut reader, request.clone())?;
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(timed_out["result"]["isError"], true);
    assert!(tool_text(&timed_out).contains("configured deadline"));

    let retry = send_request(&mut stdin, &mut reader, request)?;
    assert_eq!(retry["result"]["isError"], true);
    assert!(!tool_text(&retry).contains("still in progress"));

    let _ = child.kill();
    Ok(())
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
fn mcp_implicit_default_path_obeys_strict_allowlist() -> Result<(), Box<dyn std::error::Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn hidden() {}\n")?;

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
    cmd.env("CCM_DISABLE_EMBEDDER", "1")
        .env("CCM_MCP_DEBUG", "0")
        .env_remove("CCM_ALLOWED_ROOTS")
        .env_remove("CCM_PROJECT_ROOT")
        .env_remove("CCM_REQUIRE_ALLOWED_ROOTS")
        .current_dir(project.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let denied = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"get_context","arguments":{"file":"main.rs","line":1}}
        }),
    )?;
    assert_eq!(denied["error"]["code"], -32603);

    let _ = child.kill();
    Ok(())
}

#[test]
fn mcp_strict_mode_without_default_root_rejects_implicit_retrieval(
) -> Result<(), Box<dyn std::error::Error>> {
    let home = tempdir()?;
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
    cmd.env("CCM_DISABLE_EMBEDDER", "1")
        .env("CCM_MCP_DEBUG", "0")
        .env("CCM_REQUIRE_ALLOWED_ROOTS", "1")
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env_remove("CCM_ALLOWED_ROOTS")
        .env_remove("CCM_PROJECT_ROOT")
        .current_dir("/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let denied = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"get_context","arguments":{"file":"main.rs","line":1}}
        }),
    )?;
    assert_eq!(denied["error"]["code"], -32603);
    let message = denied["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("No default project root"),
        "unexpected strict-root error: {message}"
    );

    let _ = child.kill();
    Ok(())
}

#[test]
fn mcp_suppresses_notification_responses() -> Result<(), Box<dyn std::error::Error>> {
    let project = tempdir()?;
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
    cmd.env("CCM_DISABLE_EMBEDDER", "1")
        .env("CCM_MCP_DEBUG", "0")
        .env("CCM_PROJECT_ROOT", project.path())
        .env("CCM_ALLOWED_ROOTS", project.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    writeln!(stdin, "{}", json!({"jsonrpc":"2.0","method":"tools/list"}))?;
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc":"2.0","id":9,"method":"resources/list"})
    )?;
    stdin.flush()?;

    let mut line = String::new();
    reader.read_line(&mut line)?;
    let response: serde_json::Value = serde_json::from_str(&line)?;
    assert_eq!(response["id"], 9);
    assert!(response["result"]["resources"].is_array());

    let _ = child.kill();
    Ok(())
}

#[test]
fn mcp_suppresses_invalid_notification_errors() -> Result<(), Box<dyn std::error::Error>> {
    let project = tempdir()?;
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
    cmd.env("CCM_DISABLE_EMBEDDER", "1")
        .env("CCM_MCP_DEBUG", "0")
        .env("CCM_PROJECT_ROOT", project.path())
        .env("CCM_ALLOWED_ROOTS", project.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc":"2.0","method":"tools/call","params":{}})
    )?;
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc":"2.0","id":9,"method":"resources/list"})
    )?;
    stdin.flush()?;

    let mut line = String::new();
    reader.read_line(&mut line)?;
    let response: serde_json::Value = serde_json::from_str(&line)?;
    assert_eq!(response["id"], 9);
    assert!(response["result"]["resources"].is_array());

    let _ = child.kill();
    Ok(())
}

#[test]
fn mcp_rejects_invalid_tools_before_lazy_indexing() -> Result<(), Box<dyn std::error::Error>> {
    let server_root = tempdir()?;
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn untouched() {}\n")?;

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
    cmd.env("CCM_DISABLE_EMBEDDER", "1")
        .env("CCM_MCP_DEBUG", "0")
        .env("CCM_ALLOWED_ROOTS", project.path())
        .env_remove("CCM_PROJECT_ROOT")
        .current_dir(server_root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let unknown = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"typo_tool","arguments":{"project_path":project.path()}}
        }),
    )?;
    assert_eq!(unknown["error"]["code"], -32602);

    let malformed = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"get_context","arguments":{"project_path":project.path()}}
        }),
    )?;
    assert_eq!(malformed["error"]["code"], -32602);
    assert!(!project.path().join("data").exists());

    let _ = child.kill();
    Ok(())
}

#[test]
fn mcp_missing_index_fails_fast_without_hidden_rebuild() -> Result<(), Box<dyn std::error::Error>> {
    let server_root = tempdir()?;
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn pending() {}\n")?;

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
    cmd.env("CCM_DISABLE_EMBEDDER", "1")
        .env("CCM_MCP_DEBUG", "0")
        .env("CCM_ALLOWED_ROOTS", project.path())
        .env_remove("CCM_PROJECT_ROOT")
        .current_dir(server_root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let initialized = send_request(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}),
    )?;
    assert!(initialized.get("result").is_some());
    let started = Instant::now();
    let response = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"get_context","arguments":{
                "file":"main.rs","line":1,"project_path":project.path()
            }}
        }),
    )?;
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(response["error"]["code"], -32603);
    assert!(response["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("Call index_project first"));
    assert!(!project.path().join("data").exists());

    let _ = child.kill();
    Ok(())
}

#[test]
fn mcp_custom_db_path_is_used_for_index_and_retrieval() -> Result<(), Box<dyn std::error::Error>> {
    let project = tempdir()?;
    let custom_db = project.path().join(".ccm/db");
    fs::write(project.path().join("main.rs"), "fn custom_location() {}\n")?;

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
    cmd.env("CCM_DISABLE_EMBEDDER", "1")
        .env("CCM_MCP_DEBUG", "0")
        .env("CCM_PROJECT_ROOT", project.path())
        .env("CCM_ALLOWED_ROOTS", project.path())
        .env("CCM_DB_PATH", &custom_db)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let indexed = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"index_project","arguments":{"project_path":project.path()}}
        }),
    )?;
    assert!(tool_text(&indexed).contains("Project index refreshed successfully"));
    let artifacts = ccm_core::resolve_index_artifacts(
        project.path().to_string_lossy().as_ref(),
        Some(custom_db.to_string_lossy().as_ref()),
    )?;
    assert!(artifacts.graph_path.is_file());
    let canonical_custom_parent = project.path().canonicalize()?.join(".ccm");
    assert!(
        artifacts.db_path.starts_with(&canonical_custom_parent),
        "active custom DB '{}' is outside '{}'",
        artifacts.db_path.display(),
        canonical_custom_parent.display()
    );
    assert!(!project.path().join("data/ccm_graph.json").exists());

    let context = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"get_context","arguments":{"file":"main.rs","line":1}}
        }),
    )?;
    assert!(tool_text(&context).contains("Current: custom_location"));

    let _ = child.kill();
    Ok(())
}

#[test]
fn mcp_default_corrupt_graph_requires_and_accepts_rebuild() -> Result<(), Box<dyn std::error::Error>>
{
    let project = tempdir()?;
    fs::create_dir_all(project.path().join("data/ccm_db"))?;
    fs::write(project.path().join("main.rs"), "fn repaired() {}\n")?;
    fs::write(project.path().join("data/ccm_graph.json"), "{broken")?;
    fs::write(
        project.path().join("data/ccm_manifest.json"),
        format!(
            "{{\"schema_version\":{},\"files\":{{}}}}",
            ccm_core::INDEX_SCHEMA_VERSION
        ),
    )?;

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
    cmd.env("CCM_DISABLE_EMBEDDER", "1")
        .env("CCM_MCP_DEBUG", "0")
        .env("CCM_PROJECT_ROOT", project.path())
        .env("CCM_ALLOWED_ROOTS", project.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let rejected = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"get_context","arguments":{"file":"main.rs","line":1}}
        }),
    )?;
    assert_eq!(rejected["error"]["code"], -32603);

    let rebuilt = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"index_project","arguments":{"project_path":project.path()}}
        }),
    )?;
    assert!(tool_text(&rebuilt).contains("Project index refreshed successfully"));

    let recovered = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"get_context","arguments":{"file":"main.rs","line":1}}
        }),
    )?;
    assert!(tool_text(&recovered).contains("Current: repaired"));

    let _ = child.kill();
    Ok(())
}

#[test]
fn mcp_broken_generation_pointer_can_be_repaired_with_index_project(
) -> Result<(), Box<dyn std::error::Error>> {
    let project = tempdir()?;
    fs::create_dir_all(project.path().join("data"))?;
    fs::write(project.path().join("main.rs"), "fn pointer_repaired() {}\n")?;
    fs::write(
        project.path().join("data/ccm_current"),
        "missing-generation",
    )?;

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
    cmd.env("CCM_DISABLE_EMBEDDER", "1")
        .env("CCM_MCP_DEBUG", "0")
        .env("CCM_PROJECT_ROOT", project.path())
        .env("CCM_ALLOWED_ROOTS", project.path())
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

    let rebuilt = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"index_project","arguments":{"project_path":project.path()}}
        }),
    )?;
    assert!(tool_text(&rebuilt).contains("Project index refreshed successfully"));

    let recovered = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"get_context","arguments":{"file":"main.rs","line":1}}
        }),
    )?;
    assert!(tool_text(&recovered).contains("Current: pointer_repaired"));

    let _ = child.kill();
    Ok(())
}

#[test]
fn mcp_recovers_after_an_oversized_frame() -> Result<(), Box<dyn std::error::Error>> {
    let project = tempdir()?;
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
    cmd.env("CCM_DISABLE_EMBEDDER", "1")
        .env("CCM_MCP_DEBUG", "0")
        .env("CCM_PROJECT_ROOT", project.path())
        .env("CCM_ALLOWED_ROOTS", project.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let oversized = vec![b'x'; 10 * 1024 * 1024 + 1];
    stdin.write_all(&oversized)?;
    stdin.write_all(b"\n")?;
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc":"2.0","id":7,"method":"resources/list"})
    )?;
    stdin.flush()?;

    let mut line = String::new();
    reader.read_line(&mut line)?;
    let rejected: serde_json::Value = serde_json::from_str(&line)?;
    assert_eq!(rejected["error"]["code"], -32700);
    assert!(rejected["id"].is_null());

    line.clear();
    reader.read_line(&mut line)?;
    let recovered: serde_json::Value = serde_json::from_str(&line)?;
    assert_eq!(recovered["id"], 7);
    assert!(recovered["result"]["resources"].is_array());

    let _ = child.kill();
    Ok(())
}

#[test]
fn mcp_classifies_protocol_errors_and_continues() -> Result<(), Box<dyn std::error::Error>> {
    let project = tempdir()?;
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
    cmd.env("CCM_DISABLE_EMBEDDER", "1")
        .env("CCM_MCP_DEBUG", "0")
        .env("CCM_PROJECT_ROOT", project.path())
        .env("CCM_ALLOWED_ROOTS", project.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    stdin.write_all(b"{broken\n")?;
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc":"1.0","id":2,"method":"resources/list"})
    )?;
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc":"2.0","id":{"bad":true},"method":"resources/list"})
    )?;
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc":"2.0","id":4,"method":"resources/list","params":"bad"})
    )?;
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc":"2.0","id":3,"method":"resources/list"})
    )?;
    stdin.flush()?;

    let mut line = String::new();
    reader.read_line(&mut line)?;
    let parse_error: serde_json::Value = serde_json::from_str(&line)?;
    assert_eq!(parse_error["error"]["code"], -32700);
    assert!(parse_error["id"].is_null());

    line.clear();
    reader.read_line(&mut line)?;
    let invalid_request: serde_json::Value = serde_json::from_str(&line)?;
    assert_eq!(invalid_request["id"], 2);
    assert_eq!(invalid_request["error"]["code"], -32600);

    line.clear();
    reader.read_line(&mut line)?;
    let invalid_id: serde_json::Value = serde_json::from_str(&line)?;
    assert!(invalid_id["id"].is_null());
    assert_eq!(invalid_id["error"]["code"], -32600);

    line.clear();
    reader.read_line(&mut line)?;
    let invalid_params: serde_json::Value = serde_json::from_str(&line)?;
    assert_eq!(invalid_params["id"], 4);
    assert_eq!(invalid_params["error"]["code"], -32602);

    line.clear();
    reader.read_line(&mut line)?;
    let recovered: serde_json::Value = serde_json::from_str(&line)?;
    assert_eq!(recovered["id"], 3);
    assert!(recovered["result"]["resources"].is_array());

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
                "file":"camera.py","line":4,"project_path":project_root,
                "include_body":true
            }}
        }),
    )?;
    let context_text = tool_text(&context);
    assert!(context_text.contains("Current: open_camera"));
    assert!(context_text.contains("def open_camera"));

    // Varsayılan (metadata-only) çıktı body içermez ama node kimliği taşır.
    let context_meta = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":6,"method":"tools/call",
            "params":{"name":"get_context","arguments":{
                "file":"camera.py","line":4,"project_path":project_root
            }}
        }),
    )?;
    let context_meta_text = tool_text(&context_meta);
    assert!(context_meta_text.contains("Current: open_camera"));
    assert!(!context_meta_text.contains("def open_camera"));

    let graph = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":7,"method":"tools/call",
            "params":{"name":"read_graph","arguments":{
                "node_id":node_id,"project_path":project_root
            }}
        }),
    )?;
    let graph_text = tool_text(&graph);
    assert!(graph_text.contains("Node Details: YoloDetector"));
    assert!(!graph_text.contains("class YoloDetector"));

    let graph_with_body = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":8,"method":"tools/call",
            "params":{"name":"read_graph","arguments":{
                "node_id":node_id,"project_path":project_root,
                "include_body":true,"max_chars":8
            }}
        }),
    )?;
    let graph_with_body_text = tool_text(&graph_with_body);
    assert!(graph_with_body_text.contains("class Yo"));
    assert!(graph_with_body_text.contains("body truncated by max_chars"));

    let impact = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":9,"method":"tools/call",
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
