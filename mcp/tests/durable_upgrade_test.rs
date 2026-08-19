use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};
use tempfile::tempdir;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../eval/fixtures/embeddings.ndjson")
}

fn worker_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ccm-mcp"))
}

fn spawn_worker(args: &[&str], envs: &[(&str, String)]) -> std::process::Child {
    let mut cmd = Command::new(worker_bin());
    cmd.env("CCM_INTERNAL_INDEX_WORKER", "1")
        .stdin(std::process::Stdio::null());
    for (key, value) in envs {
        cmd.env(key, value);
    }
    cmd.args(args);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // `spawn_detached_upgrade_worker` ile aynı: süreci kendi process grubuna al.
        cmd.process_group(0);
    }
    cmd.spawn().expect("worker should spawn")
}

/// Quick (graph-only) index sonrası semantic upgrade worker'ının detached
/// mekanizmayla (kendi process grubu, kill_on_drop yok) tamamlandığını ve yeni
/// generation'a `code_vectors.lance` kurduğunu doğrular. Bu, `schedule_semantic_
/// upgrade`'in MCP sürecinden bağımsız taze vektör üretebilmesinin regresyon
/// guard'ıdır.
#[test]
fn detached_semantic_upgrade_installs_vector_table() -> Result<(), Box<dyn std::error::Error>> {
    let temp_root = tempdir()?;
    let repo_root = temp_root.path().join("repo_a");
    let fixture_src =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../eval/fixtures/repos/repo_a/src");
    copy_tree(&fixture_src, &repo_root.join("src"))?;

    let repo_str = repo_root.to_string_lossy().to_string();
    let db_str = repo_root.join("data/ccm_db").to_string_lossy().to_string();

    // 1) Quick index: graph-only, vektör tablosu üretilmez.
    let mut quick = spawn_worker(
        &["--ccm-internal-index-worker", &repo_str, &db_str, "quick"],
        &[("CCM_DISABLE_EMBEDDER", "1".to_string())],
    );
    assert!(quick.wait()?.success(), "quick worker should succeed");
    assert!(
        !active_vector_table_exists(&repo_root),
        "quick index must not create a vector table before upgrade"
    );

    // 2) Detached upgrade: worker kendi process grubunda koşar (kill_on_drop yok),
    //    bu yüzden MCP çıkışında işi bırakmaz. Bu test worker'ın tamamlanıp yeni
    //    generate vektör tablosunu kurduğunu doğrular; parent-ölüm dayanıklılığı
    //    `process_group(0)` + kill_on_drop yokluğuyla sağlanır.
    let fixture = fixture_path().to_string_lossy().to_string();
    let mut upgrade = spawn_worker(
        &["--ccm-internal-index-worker", &repo_str, &db_str, "upgrade"],
        &[("CCM_EMBEDDING_FIXTURE", fixture)],
    );

    let mut installed = false;
    let deadline = Instant::now() + Duration::from_secs(120);
    while Instant::now() < deadline {
        if active_vector_table_exists(&repo_root) {
            installed = true;
            break;
        }
        if let Ok(Some(status)) = upgrade.try_wait() {
            if status.success() && active_vector_table_exists(&repo_root) {
                installed = true;
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = upgrade.kill();
    let _ = upgrade.wait();
    if installed {
        return Ok(());
    }
    let current = repo_root
        .join("data/ccm_current")
        .to_string_lossy()
        .to_string();
    let gens = repo_root.join("data/.ccm-generations");
    let entries: Vec<String> = fs::read_dir(&gens)
        .unwrap_or_else(|_| fs::read_dir(repo_root.join("data")).unwrap())
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    let pointer = fs::read_to_string(repo_root.join("data/ccm_current")).unwrap_or_default();
    let mut tables: Vec<String> = Vec::new();
    for gen in &entries {
        let db = gens.join(gen).join("ccm_db");
        let has = db.join("code_vectors.lance").exists();
        tables.push(format!("{}@{}", gen, has));
    }
    Err(format!(
        "detached upgrade never installed code_vectors.lance (current={} pointer={:?} gens={:?} tables={:?})",
        current, pointer, entries, tables
    )
    .into())
}

fn active_vector_table_exists(repo_root: &std::path::Path) -> bool {
    let current = fs::read_to_string(repo_root.join("data").join("ccm_current"))
        .unwrap_or_default()
        .trim()
        .to_string();
    if current.trim().is_empty() {
        return false;
    }
    repo_root
        .join("data")
        .join(".ccm-generations")
        .join(current)
        .join("ccm_db")
        .join("code_vectors.lance")
        .exists()
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    if from.is_file() {
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(from, to)?;
        return Ok(());
    }
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        copy_tree(&entry.path(), &to.join(entry.file_name()))?;
    }
    Ok(())
}
