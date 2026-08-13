use anyhow::{Context, Result};
use git2::{Repository, Status, StatusOptions};
use std::path::{Path, PathBuf};

/// Integrates with the local Git repository to detecting changes.
pub struct GitIntegrator {
    repo: Repository,
}

impl GitIntegrator {
    /// Opens the Git repository at the given path.
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let repo = Repository::open(path).context("Failed to open Git repository")?;
        Ok(Self { repo })
    }

    /// Returns a list of files that have changed since HEAD (staged or unstaged).
    /// Returns absolute paths (including deleted files).
    pub fn get_changed_files(&self) -> Result<Vec<PathBuf>> {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true).recurse_untracked_dirs(true);

        let statuses = self.repo.statuses(Some(&mut opts))?;
        let mut changed_files = Vec::new();
        let workdir = self
            .repo
            .workdir()
            .context("Repository has no working directory")?;

        for entry in statuses.iter() {
            let status = entry.status();

            // Check for relevant changes (Modified, Added, Renamed, Typechange)
            // We ignore Deleted files here (handled by removal logic usually)
            // But for incremental index, we need to know what to re-parse.
            // Deleted files won't exist on disk, so we can't parse them.
            // We should filter them out for "re-indexing", but might need them for "removal".
            // For now, let's return files that physically exist and are changed.

            if status.contains(Status::INDEX_NEW)
                || status.contains(Status::INDEX_MODIFIED)
                || status.contains(Status::INDEX_RENAMED)
                || status.contains(Status::INDEX_DELETED)
                || status.contains(Status::WT_NEW)
                || status.contains(Status::WT_MODIFIED)
                || status.contains(Status::WT_RENAMED)
                || status.contains(Status::WT_DELETED)
            {
                let path_str = match entry.path() {
                    Ok(path) => path,
                    Err(error) => {
                        tracing::warn!(
                            error = %error,
                            "UTF-8 olmayan Git status yolu atlandı"
                        );
                        continue;
                    }
                };
                let full_path = workdir.join(path_str);
                if full_path.exists()
                    || status.contains(Status::WT_DELETED)
                    || status.contains(Status::INDEX_DELETED)
                {
                    changed_files.push(full_path);
                }
            }
        }

        Ok(changed_files)
    }

    /// Son N günde commit edilen dosyaları döndürür.
    pub fn get_changed_files_since_days(&self, days: u32) -> Result<Vec<PathBuf>> {
        let workdir = self
            .repo
            .workdir()
            .context("Repository has no working directory")?
            .to_path_buf();

        let cutoff = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
            - (days as i64 * 86400);

        let mut revwalk = self.repo.revwalk()?;
        revwalk.push_head()?;
        revwalk.set_sorting(git2::Sort::TIME)?;

        let mut changed: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

        for oid_result in revwalk {
            let oid = oid_result?;
            let commit = self.repo.find_commit(oid)?;

            if commit.time().seconds() < cutoff {
                break;
            }

            if commit.parent_count() == 0 {
                let tree = commit.tree()?;
                tree.walk(git2::TreeWalkMode::PreOrder, |root, entry| {
                    if entry.kind() == Some(git2::ObjectType::Blob) {
                        let rel = if root.is_empty() {
                            PathBuf::from(entry.name().unwrap_or(""))
                        } else {
                            PathBuf::from(root).join(entry.name().unwrap_or(""))
                        };
                        changed.insert(workdir.join(rel));
                    }
                    git2::TreeWalkResult::Ok
                })?;
            } else {
                let parent_tree = commit.parent(0)?.tree()?;
                let tree = commit.tree()?;
                let diff = self
                    .repo
                    .diff_tree_to_tree(Some(&parent_tree), Some(&tree), None)?;
                diff.foreach(
                    &mut |delta, _| {
                        if let Some(p) = delta.new_file().path() {
                            changed.insert(workdir.join(p));
                        }
                        true
                    },
                    None,
                    None,
                    None,
                )?;
            }
        }

        Ok(changed.into_iter().collect())
    }
}
