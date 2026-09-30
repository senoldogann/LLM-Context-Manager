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

/// `~/.ccm/.env` komut çalışmadan önce yüklenir: oradaki `CCM_DISABLE_EMBEDDER`
/// indeksi graf-yalnız kurar (model indirilmez, sonraki indeks onu model
/// değişikliği sayıp yeniden kurmaz), `CCM_MODEL_DIR` ve `HF_ENDPOINT`
/// `models pull`'un dizinini ve kaynağını belirler, doktor kapalı embedder'ı görür.
#[test]
fn user_env_file_configures_the_cli_at_startup() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir()?;
    let project_root = dir.path().join("project");
    let home = dir.path().join("home");
    let models = dir.path().join("models-from-env");
    fs::create_dir_all(&project_root)?;
    fs::create_dir_all(home.join(".ccm"))?;
    fs::write(project_root.join("main.rs"), "fn existing_symbol() {}\n")?;
    // Kapalı port: indirme denenirse hızla başarısız olur.
    fs::write(
        home.join(".ccm/.env"),
        format!(
            "CCM_DISABLE_EMBEDDER=1\nCCM_MODEL_DIR={}\nHF_ENDPOINT=http://127.0.0.1:9\n",
            models.display()
        ),
    )?;
    let project = project_root.to_string_lossy().to_string();
    let ccm = |args: &[&str]| {
        Command::new(assert_cmd::cargo::cargo_bin!("ccm-cli"))
            .current_dir(&project_root)
            .env("HOME", &home)
            .env("CCM_PROJECT_ROOT", &project_root)
            .env_remove("CCM_DISABLE_EMBEDDER")
            .env_remove("EMBEDDING_DISABLED")
            .env_remove("CCM_EMBEDDING_FIXTURE")
            .env_remove("EMBEDDING_PROVIDER")
            .env_remove("EMBEDDING_HOST")
            .env_remove("EMBEDDING_MODEL")
            .env_remove("CCM_MODEL_DIR")
            .env_remove("HF_ENDPOINT")
            .args(args)
            .output()
    };

    let first = ccm(&["index", "--path", &project])?;
    assert!(
        first.status.success(),
        "index failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(
        !models.exists() && !home.join(".ccm/models").exists(),
        "a disabled embedder must not download the model"
    );
    let artifacts = ccm_core::resolve_index_artifacts(&project, None)?;
    assert_eq!(
        ccm_core::read_index_embedding(&artifacts.manifest_path)?,
        None
    );
    let second = ccm(&["index", "--path", &project])?;
    assert!(second.status.success());
    assert_eq!(
        ccm_core::resolve_index_artifacts(&project, None)?.generation_id,
        artifacts.generation_id,
        "an unchanged graph-only index is not rebuilt"
    );

    let doctor = ccm(&["doctor", "--path", &project, "--json"])?;
    let report: serde_json::Value = serde_json::from_slice(&doctor.stdout)?;
    assert_eq!(report["checks"]["embedding"]["disabled"], true, "{report}");

    let pull = ccm(&["models", "pull"])?;
    assert!(
        !pull.status.success(),
        "the closed HF_ENDPOINT port must fail the download"
    );
    let pull_stdout = String::from_utf8_lossy(&pull.stdout);
    assert!(
        pull_stdout.contains(&models.display().to_string()),
        "models pull must use CCM_MODEL_DIR from ~/.ccm/.env: {pull_stdout}"
    );
    assert!(
        String::from_utf8_lossy(&pull.stderr).contains("127.0.0.1:9"),
        "models pull must use HF_ENDPOINT from ~/.ccm/.env"
    );
    Ok(())
}

/// Gerçek yerel modelle uçtan uca: hiçbir embedding ayarı yokken `index`
/// semantik indeks kurar, manifest yerel modeli kaydeder ve `query` sonucu
/// semantik skor taşır. ~120 MB model indirir (önbellek: `~/.ccm/models`);
/// `CCM_TEST_LOCAL_MODEL=1 cargo test -p ccm-cli -- --ignored local_model` ile çalışır.
#[test]
#[ignore = "downloads the ~120 MB local model; set CCM_TEST_LOCAL_MODEL=1"]
fn local_model_indexes_and_searches_without_embedding_configuration(
) -> Result<(), Box<dyn std::error::Error>> {
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
    // Kullanıcının `~/.ccm/.env` dosyası yüklenmesin diye HOME yalıtılır; model
    // önbelleği yine paylaşılan dizindedir.
    let model_dir = ccm_core::vector::local_model::models_root()?;
    let ccm = |args: &[&str]| {
        Command::new(assert_cmd::cargo::cargo_bin!("ccm-cli"))
            .current_dir(&project_root)
            .env("HOME", &isolated_home)
            .env("CCM_MODEL_DIR", &model_dir)
            .env_remove("CCM_DISABLE_EMBEDDER")
            .env_remove("EMBEDDING_DISABLED")
            .env_remove("CCM_EMBEDDING_FIXTURE")
            .env_remove("EMBEDDING_PROVIDER")
            .env_remove("EMBEDDING_HOST")
            .env_remove("EMBEDDING_MODEL")
            .args(args)
            .output()
    };

    let index = ccm(&["index", "--path", project_root.to_string_lossy().as_ref()])?;
    assert!(
        index.status.success(),
        "index failed: {}",
        String::from_utf8_lossy(&index.stderr)
    );
    let artifacts = ccm_core::resolve_index_artifacts(&project_root.to_string_lossy(), None)?;
    let identity = ccm_core::read_index_embedding(&artifacts.manifest_path)?
        .ok_or("the index must record the local model identity")?;
    assert_eq!(
        (identity.provider.as_str(), identity.dim),
        ("local", 384),
        "{identity}"
    );

    let query = ccm(&[
        "query",
        "--text",
        "where is the tax of an invoice calculated",
    ])?;
    let stdout = String::from_utf8_lossy(&query.stdout);
    assert!(query.status.success(), "query failed: {stdout}");
    let first = stdout
        .split("\n#")
        .find(|block| block.contains("Reason:"))
        .ok_or_else(|| format!("no results: {stdout}"))?;
    assert!(first.contains("compute_invoice_tax"), "{stdout}");
    assert!(
        !first.contains("semantic 0.00"),
        "the top hit must carry a semantic score: {stdout}"
    );
    Ok(())
}
