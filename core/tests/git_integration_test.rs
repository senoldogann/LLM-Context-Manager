use anyhow::Result;
use ccm_core::git::GitIntegrator;
use git2::{Repository, Signature, Time};
use std::collections::HashSet;
use std::path::Path;
use tempfile::{tempdir, TempDir};

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("sistem saati UNIX epoch öncesi olamaz")
        .as_secs() as i64
}

fn signature_at(seconds: i64) -> Result<Signature<'static>> {
    Ok(Signature::new(
        "Coverage Test",
        "coverage@example.com",
        &Time::new(seconds, 0),
    )?)
}

fn write_file(workdir: &Path, name: &str, content: &str) -> Result<()> {
    let path = workdir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)?;
    Ok(())
}

fn init_repo() -> Result<(TempDir, Repository)> {
    let dir = tempdir()?;
    let repo = Repository::init(dir.path())?;
    Ok((dir, repo))
}

fn commit_files(repo: &Repository, message: &str, when: i64, files: &[(&str, &str)]) -> Result<()> {
    let workdir = repo.workdir().expect("commit için workdir gerekir");
    for (name, content) in files {
        write_file(workdir, name, content)?;
    }
    let mut index = repo.index()?;
    for (name, _) in files {
        index.add_path(Path::new(name))?;
    }
    index.write()?;
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;
    let signature = signature_at(when)?;
    let parent = repo.head().ok().and_then(|head| head.peel_to_commit().ok());
    let parents: Vec<&git2::Commit> = parent.iter().collect();
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        message,
        &tree,
        &parents,
    )?;
    Ok(())
}

fn changed_names(integrator: &GitIntegrator) -> Result<HashSet<String>> {
    let changed = integrator.get_changed_files()?;
    Ok(changed
        .into_iter()
        .filter_map(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .collect())
}

#[test]
fn changed_files_include_all_staged_and_worktree_states() -> Result<()> {
    let (_dir, repo) = init_repo()?;
    commit_files(
        &repo,
        "initial",
        now_secs() - 60,
        &[
            ("a.rs", "fn a() {}\n"),
            ("del_work.rs", "fn deleted_from_worktree() {}\n"),
            ("del_index.rs", "fn deleted_from_index() {}\n"),
        ],
    )?;
    let workdir = repo.workdir().expect("workdir");

    write_file(workdir, "a.rs", "fn a_updated() {}\n")?;
    std::fs::remove_file(workdir.join("del_work.rs"))?;
    write_file(workdir, "staged_new.rs", "fn staged_new() {}\n")?;
    write_file(workdir, "untracked.rs", "fn untracked() {}\n")?;

    let mut index = repo.index()?;
    index.add_path(Path::new("staged_new.rs"))?;
    index.remove_path(Path::new("del_index.rs"))?;
    index.write()?;

    let names = changed_names(&GitIntegrator::new(repo.workdir().expect("workdir"))?)?;
    for expected in [
        "a.rs",
        "del_work.rs",
        "del_index.rs",
        "staged_new.rs",
        "untracked.rs",
    ] {
        assert!(names.contains(expected), "eksik değişiklik: {expected}");
    }
    Ok(())
}

#[test]
fn staged_rename_surfaces_as_new_and_deleted_paths() -> Result<()> {
    let (_dir, repo) = init_repo()?;
    commit_files(
        &repo,
        "initial",
        now_secs() - 60,
        &[("old_name.rs", "fn renamed() {}\n")],
    )?;
    let workdir = repo.workdir().expect("workdir");

    std::fs::rename(workdir.join("old_name.rs"), workdir.join("new_name.rs"))?;
    let mut index = repo.index()?;
    index.remove_path(Path::new("old_name.rs"))?;
    index.add_path(Path::new("new_name.rs"))?;
    index.write()?;

    let names = changed_names(&GitIntegrator::new(repo.workdir().expect("workdir"))?)?;
    assert!(names.contains("old_name.rs"));
    assert!(names.contains("new_name.rs"));
    Ok(())
}

#[test]
fn changed_files_since_days_filters_by_commit_window() -> Result<()> {
    let (_dir, repo) = init_repo()?;
    let now = now_secs();
    commit_files(
        &repo,
        "old",
        now - 3 * 86400,
        &[("old.rs", "fn old() {}\n")],
    )?;
    commit_files(
        &repo,
        "recent",
        now - 3600,
        &[("recent.rs", "fn recent() {}\n")],
    )?;
    let integrator = GitIntegrator::new(repo.workdir().expect("workdir"))?;

    let two_days: HashSet<String> = integrator
        .get_changed_files_since_days(2)?
        .into_iter()
        .filter_map(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .collect();
    assert!(two_days.contains("recent.rs"));
    assert!(!two_days.contains("old.rs"));

    let ten_days: HashSet<String> = integrator
        .get_changed_files_since_days(10)?
        .into_iter()
        .filter_map(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .collect();
    assert!(ten_days.contains("recent.rs"));
    assert!(ten_days.contains("old.rs"));
    Ok(())
}

#[test]
fn changed_files_since_days_includes_initial_commit_tree() -> Result<()> {
    let (_dir, repo) = init_repo()?;
    commit_files(
        &repo,
        "ilk commit",
        now_secs() - 3600,
        &[
            ("src/main.rs", "fn main() {}\n"),
            ("src/lib.rs", "fn lib() {}\n"),
        ],
    )?;
    let changed =
        GitIntegrator::new(repo.workdir().expect("workdir"))?.get_changed_files_since_days(2)?;
    assert_eq!(changed.len(), 2);
    Ok(())
}

#[test]
fn integrator_rejects_missing_path_and_bare_repository() -> Result<()> {
    let missing = tempdir()?;
    assert!(GitIntegrator::new(missing.path().join("not_a_repo")).is_err());

    let bare_dir = tempdir()?;
    Repository::init_bare(bare_dir.path())?;
    let integrator = GitIntegrator::new(bare_dir.path())?;
    assert!(integrator.get_changed_files().is_err());
    Ok(())
}
