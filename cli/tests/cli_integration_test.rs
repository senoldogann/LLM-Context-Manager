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
