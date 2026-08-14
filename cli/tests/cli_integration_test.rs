use std::fs;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn cli_index_then_query_file_line() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir()?;
    let project_root = dir.path();

    fs::write(project_root.join("main.rs"), "fn foo() {}\n")?;
    fs::create_dir_all(project_root.join("data"))?;

    let status = Command::new(assert_cmd::cargo::cargo_bin!("ccm-cli"))
        .env("CCM_DISABLE_EMBEDDER", "1")
        .arg("index")
        .arg("--path")
        .arg(project_root)
        .arg("--db-path")
        .arg(project_root.join("data/ccm_db"))
        .status()?;
    assert!(status.success());

    let output = Command::new(assert_cmd::cargo::cargo_bin!("ccm-cli"))
        .env("CCM_DISABLE_EMBEDDER", "1")
        .current_dir(project_root)
        .arg("query")
        .arg("--text")
        .arg("main.rs:1")
        .output()?;
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Current:"));

    Ok(())
}

#[test]
fn doctor_rejects_a_corrupt_graph() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir()?;
    let project_root = dir.path();
    fs::write(project_root.join("main.rs"), "fn healthy() {}\n")?;
    let indexed = Command::new(assert_cmd::cargo::cargo_bin!("ccm-cli"))
        .env("CCM_DISABLE_EMBEDDER", "1")
        .arg("index")
        .arg("--path")
        .arg(project_root)
        .status()?;
    assert!(indexed.success());

    let artifacts =
        ccm_core::resolve_index_artifacts(project_root.to_string_lossy().as_ref(), None)?;
    fs::write(artifacts.graph_path, "{broken")?;
    let output = Command::new(assert_cmd::cargo::cargo_bin!("ccm-cli"))
        .env("CCM_DISABLE_EMBEDDER", "1")
        .env("CCM_PROJECT_ROOT", project_root)
        .arg("doctor")
        .arg("--path")
        .arg(project_root)
        .arg("--json")
        .output()?;

    assert!(!output.status.success());
    let stdout: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(stdout["healthy"], false);
    assert_eq!(stdout["checks"]["graph"]["ok"], false);
    assert!(stdout["checks"]["graph"]["error"].is_string());

    Ok(())
}

#[test]
fn doctor_rejects_semantic_graph_without_vectors() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir()?;
    let project_root = dir.path();
    fs::write(project_root.join("main.rs"), "fn semantic_node() {}\n")?;
    let indexed = Command::new(assert_cmd::cargo::cargo_bin!("ccm-cli"))
        .env("CCM_DISABLE_EMBEDDER", "1")
        .arg("index")
        .arg("--path")
        .arg(project_root)
        .status()?;
    assert!(indexed.success());

    let output = Command::new(assert_cmd::cargo::cargo_bin!("ccm-cli"))
        .env_remove("CCM_DISABLE_EMBEDDER")
        .env_remove("EMBEDDING_DISABLED")
        .env("CCM_PROJECT_ROOT", project_root)
        .arg("doctor")
        .arg("--path")
        .arg(project_root)
        .arg("--json")
        .output()?;

    assert!(!output.status.success());
    let stdout: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|error| {
        format!(
            "doctor JSON parse failed: {}; stderr: {}; exit: {:?}",
            error,
            String::from_utf8_lossy(&output.stderr),
            output.status.code()
        )
    })?;
    assert_eq!(stdout["healthy"], false);
    assert_eq!(stdout["checks"]["vector_index"]["ok"], false);
    assert!(
        stdout["checks"]["graph"]["semantic_nodes"]
            .as_u64()
            .unwrap_or_default()
            > 0
    );
    Ok(())
}
