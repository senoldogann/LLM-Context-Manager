use serde_json::json;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use tempfile::tempdir;

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
    assert!(line.contains("Indexing completed successfully"));
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
