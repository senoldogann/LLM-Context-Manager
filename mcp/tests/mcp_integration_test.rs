mod common;

use common::{index_with_model, start_embed_server};
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

/// Araç çağrısı başarılı mı: ne JSON-RPC hatası ne de `isError: true` sonucu var.
fn tool_succeeded(response: &serde_json::Value) -> bool {
    response.get("error").is_none() && response["result"]["isError"] != true
}

/// Araç yürütme hatası JSON-RPC hatası olarak değil, `isError: true` sonucu
/// olarak döner; metin beklenen ifadeyi içerir.
fn assert_tool_error(response: &serde_json::Value, expected: &str) {
    assert!(
        response.get("error").is_none(),
        "tool failures must not be JSON-RPC errors: {response}"
    );
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert!(
        tool_text(response).contains(expected),
        "tool error must mention '{expected}': {response}"
    );
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
    assert_tool_error(&retrieval, "indexing is in progress");

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
    let response: serde_json::Value = serde_json::from_str(&line)?;
    assert_tool_error(&response, "Project path is not allowed");

    let _ = child.kill();

    Ok(())
}

#[test]
fn mcp_symlinked_data_dir_never_writes_outside_the_allowlist(
) -> Result<(), Box<dyn std::error::Error>> {
    let allowed_dir = tempdir()?;
    let project_root = allowed_dir.path().join("repo");
    let outside_dir = tempdir()?;

    fs::create_dir_all(&project_root)?;
    fs::write(project_root.join("main.rs"), "fn secret_business() {}\n")?;
    // Proje içindeki `data` dizini allowlist DIŞINI gösteren bir symlink.
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside_dir.path(), project_root.join("data"))?;

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
            "arguments": { "project_path": project_root }
        }
    });
    writeln!(stdin, "{}", index_req)?;
    stdin.flush()?;
    reader.read_line(&mut line)?;
    let response: serde_json::Value = serde_json::from_str(&line)?;
    // Symlink'li data dizini güvenli çözümleme hatası üretmeli.
    assert_tool_error(&response, "resolved safely");

    // Allowlist dışındaki dizine hiçbir index artifact'i yazılmamalı.
    assert!(
        !outside_dir.path().join("ccm_current").exists(),
        "index pointer allowlist dışına yazıldı"
    );
    assert!(
        !outside_dir.path().join("ccm_db").exists(),
        "vector DB allowlist dışına yazıldı"
    );
    assert!(
        !outside_dir.path().join(".ccm-generations").exists(),
        "generation artifacts allowlist dışına yazıldı"
    );

    child.kill()?;
    let _ = child.wait();
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
    assert_tool_error(&denied, "not allowed");

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
    assert_tool_error(&denied, "not allowed");

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
    assert_tool_error(&denied, "not allowed");

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
    assert_tool_error(&denied, "No default project root");

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
    // Argüman doğrulama hatası araç yürütme hatasıdır (MCP 2025-11-25).
    assert_tool_error(&malformed, "Missing or invalid 'file' argument");
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
    assert_tool_error(&response, "Call index_project first");
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
    // Asıl sebep ve düzeltme yolu modele gösterilir.
    assert_tool_error(&rejected, "could not be loaded");
    assert!(
        tool_text(&rejected).contains("Run index_project to rebuild it"),
        "{rejected}"
    );

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
fn mcp_quick_index_returns_synchronously_and_search_falls_back_to_graph(
) -> Result<(), Box<dyn std::error::Error>> {
    let project = tempdir()?;
    fs::write(
        project.path().join("main.rs"),
        "pub fn quick_compute_tax(base: f64) -> f64 { base * 1.24 }\n",
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
    send_request(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    )?;

    // index_now + mode quick: embedding atlanır, sonuç eşzamanlı döner.
    let indexed = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"index_now","arguments":{
                "project_path":project.path(),"mode":"quick"
            }}
        }),
    )?;
    assert!(tool_succeeded(&indexed), "index_now quick: {indexed}");
    let text = tool_text(&indexed);
    assert!(
        text.contains("graph-only"),
        "quick index_now sonucu graph-only vurgusu içermeli: {text}"
    );

    // Bozuk/oluşturulmamış vektör tablosu 500 değil graph fallback döndürür.
    let search = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"search_code","arguments":{
                "query":"compute_tax","project_path":project.path()
            }}
        }),
    )?;
    assert!(
        tool_succeeded(&search),
        "search_code bozuk vektör tablosunda hata değil graph fallback döndürmeli: {search}"
    );
    assert!(tool_text(&search).contains("quick_compute_tax"));

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

#[test]
fn mcp_answers_ping_with_empty_result() -> Result<(), Box<dyn std::error::Error>> {
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
    // MCP spec: ping isteği boş bir result ile yanıtlanmalıdır.
    let response = send_request(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc":"2.0","id":"ping-1","method":"ping"}),
    )?;
    assert_eq!(response["id"], "ping-1");
    assert_eq!(response["result"], json!({}));
    assert!(response.get("error").is_none());

    let _ = child.kill();
    Ok(())
}

#[test]
fn mcp_client_roots_select_and_allow_the_workspace() -> Result<(), Box<dyn std::error::Error>> {
    // Kurulumda başka bir dizine sabitlenmiş allowlist olsa bile istemcinin
    // MCP roots ile bildirdiği çalışma alanı varsayılan kök olarak kullanılır.
    let pinned = tempdir()?;
    let workspace = tempdir()?;
    fs::write(workspace.path().join("main.rs"), "fn workspace_only() {}\n")?;

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
    cmd.env("CCM_DISABLE_EMBEDDER", "1")
        .env("CCM_MCP_DEBUG", "0")
        .env("CCM_REQUIRE_ALLOWED_ROOTS", "1")
        .env("CCM_ALLOWED_ROOTS", pinned.path())
        .env_remove("CCM_PROJECT_ROOT")
        .current_dir("/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());

    let initialized = send_request(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize",
               "params":{"protocolVersion":"2025-11-25","capabilities":{"roots":{"listChanged":true}}}}),
    )?;
    assert!(initialized.get("result").is_some());

    // initialized bildirimi sonrası sunucu roots/list isteği göndermelidir.
    let roots_request = send_request(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )?;
    assert_eq!(roots_request["method"], "roots/list");
    let workspace_uri = url::Url::from_directory_path(workspace.path())
        .expect("absolute workspace path")
        .to_string();
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc":"2.0","id":roots_request["id"],
               "result":{"roots":[{"uri":workspace_uri,"name":"workspace"}]}})
    )?;

    let indexed = send_request(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
               "params":{"name":"index_now","arguments":{
                   "project_path":workspace.path().to_string_lossy()}}}),
    )?;
    assert!(tool_succeeded(&indexed), "index_now failed: {indexed}");

    let context = send_request(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
               "params":{"name":"get_context","arguments":{"file":"main.rs","line":1}}}),
    )?;
    assert!(
        tool_text(&context).contains("workspace_only"),
        "default root did not follow client roots: {context}"
    );

    let _ = child.kill();
    Ok(())
}

#[test]
fn mcp_serves_the_active_generation_while_reindexing() -> Result<(), Box<dyn std::error::Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn stable_symbol() {}\n")?;
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
    cmd.env("CCM_DISABLE_EMBEDDER", "1")
        .env("CCM_MCP_DEBUG", "0")
        .env("CCM_AUTO_REFRESH", "0")
        .env("CCM_INDEX_RESPONSE_TIMEOUT_MS", "1")
        .env("CCM_INTERNAL_INDEX_TEST_DELAY_MS", "3000")
        .env("CCM_PROJECT_ROOT", project.path())
        .env("CCM_ALLOWED_ROOTS", project.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    send_request(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}),
    )?;
    let indexed = send_request(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"index_now","arguments":{"project_path": project.path()}}}),
    )?;
    assert!(tool_text(&indexed).contains("Project index refreshed successfully"));

    fs::write(project.path().join("extra.rs"), "fn added_later() {}\n")?;
    let started = send_request(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"index_project","arguments":{"project_path": project.path()}}}),
    )?;
    assert!(tool_text(&started).contains("started in the background"));

    let retrieval = send_request(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"find_nodes","arguments":{"query":"stable_symbol"}}}),
    )?;
    assert!(
        tool_succeeded(&retrieval),
        "reads must keep serving the active generation: {retrieval}"
    );
    assert!(tool_text(&retrieval).contains("stable_symbol (Score:"));

    let _ = child.kill();
    Ok(())
}

/// Kök dizinsiz sunucu (proje kökü yok, depo `CCM_DB_PATH` ile verilir) başka
/// bir embedding modeliyle kurulmuş vektörleri sorgu vektörüyle karşılaştırmaz:
/// arama graf sonuçlarına döner ve neden sonucun başındaki satırda görünür.
#[test]
fn rootless_engine_reports_vectors_of_another_embedding_model(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir()?;
    let project = dir.path().join("project");
    let home = dir.path().join("home");
    fs::create_dir_all(&project)?;
    fs::create_dir_all(&home)?;
    fs::write(project.join("main.rs"), "fn existing_symbol() {}\n")?;
    let host = start_embed_server(&[])?;
    index_with_model(&project, &host, "ccm-test-embed-a")?;
    let artifacts = ccm_core::resolve_index_artifacts(&project.to_string_lossy(), None)?;

    // Başlatma dizini ev dizini olduğundan örtük kök de seçilmez.
    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"))
        .current_dir(&home)
        .env("HOME", &home)
        .env_remove("CCM_PROJECT_ROOT")
        .env_remove("CCM_ALLOWED_ROOTS")
        .env("CCM_REQUIRE_ALLOWED_ROOTS", "0")
        .env("CCM_DB_PATH", &artifacts.db_path)
        .env("CCM_MCP_DEBUG", "0")
        .env("CCM_DISABLE_EMBEDDER", "0")
        .env_remove("EMBEDDING_DISABLED")
        .env_remove("CCM_EMBEDDING_FIXTURE")
        .env("EMBEDDING_PROVIDER", "ollama")
        .env("EMBEDDING_HOST", &host)
        .env("EMBEDDING_MODEL", "ccm-test-embed-b")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdin = child.stdin.take().ok_or("stdin")?;
    let mut reader = BufReader::new(child.stdout.take().ok_or("stdout")?);
    send_request(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}),
    )?;
    let searched = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"search_code","arguments":{"query":"existing_symbol"}}
        }),
    )?;
    child.kill()?;
    child.wait()?;

    assert!(tool_succeeded(&searched), "{searched}");
    let text = tool_text(&searched);
    assert!(
        text.lines().next().is_some_and(|line| {
            line.contains("semantic search unavailable")
                && line.contains("ccm-test-embed-a")
                && line.contains("ccm-test-embed-b")
        }),
        "the reason must lead the result: {text}"
    );
    assert!(
        text.contains("existing_symbol"),
        "search falls back to graph results: {text}"
    );
    Ok(())
}

/// Gerçek yerel modelle MCP uçtan uca: ayarsız sunucu `index_now` ile semantik
/// indeks kurar, `search_code` semantik skorlu sonuç döndürür. ~120 MB model
/// indirir; `CCM_TEST_LOCAL_MODEL=1 cargo test -p ccm-mcp -- --ignored local_model`.
#[test]
#[ignore = "downloads the ~120 MB local model; set CCM_TEST_LOCAL_MODEL=1"]
fn local_model_serves_semantic_search_over_mcp() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("CCM_TEST_LOCAL_MODEL").as_deref() != Ok("1") {
        return Ok(());
    }
    let dir = tempdir()?;
    let project_root = dir.path().join("project");
    let isolated_home = dir.path().join("home");
    fs::create_dir_all(&project_root)?;
    fs::create_dir_all(&isolated_home)?;
    fs::write(
        project_root.join("billing.rs"),
        "/// Computes the tax owed on an invoice.\npub fn compute_invoice_tax(amount: f64, rate: f64) -> f64 {\n    amount * rate\n}\n",
    )?;
    fs::write(
        project_root.join("network.rs"),
        "pub fn open_tcp_connection(host: &str, port: u16) -> std::io::Result<std::net::TcpStream> {\n    std::net::TcpStream::connect((host, port))\n}\n",
    )?;
    let model_dir = std::env::var_os("CCM_MODEL_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|home| std::path::PathBuf::from(home).join(".ccm").join("models"))
        })
        .ok_or("HOME or CCM_MODEL_DIR is required for the model cache")?;

    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"))
        .env("HOME", &isolated_home)
        .env("CCM_MODEL_DIR", &model_dir)
        .env_remove("CCM_DISABLE_EMBEDDER")
        .env_remove("EMBEDDING_DISABLED")
        .env_remove("CCM_EMBEDDING_FIXTURE")
        .env_remove("EMBEDDING_PROVIDER")
        .env_remove("EMBEDDING_HOST")
        .env_remove("EMBEDDING_MODEL")
        .env("CCM_MCP_DEBUG", "0")
        .env("CCM_PROJECT_ROOT", &project_root)
        .env("CCM_ALLOWED_ROOTS", &project_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdin = child.stdin.take().ok_or("stdin")?;
    let mut reader = BufReader::new(child.stdout.take().ok_or("stdout")?);
    send_request(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}),
    )?;
    let indexed = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"index_now","arguments":{
                "project_path": project_root.to_string_lossy()
            }}
        }),
    )?;
    let index_text = tool_text(&indexed).to_string();
    assert!(
        index_text.contains("Chunks Embedded: 2"),
        "index_now must embed both functions: {index_text}"
    );
    let searched = send_request(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"search_code","arguments":{
                "query":"where is the tax of an invoice calculated",
                "project_path": project_root.to_string_lossy()
            }}
        }),
    )?;
    let search_text = tool_text(&searched).to_string();
    let first = search_text
        .split("\n## ")
        .find(|block| block.contains("**Reason:**"))
        .ok_or_else(|| format!("no results: {search_text}"))?;
    assert!(first.contains("compute_invoice_tax"), "{search_text}");
    assert!(!first.contains("semantic 0.00"), "{search_text}");
    child.kill()?;
    Ok(())
}
