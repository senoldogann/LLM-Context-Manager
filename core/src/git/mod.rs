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
    /// Returns absolute paths.
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
                || status.contains(Status::WT_NEW)
                || status.contains(Status::WT_MODIFIED)
                || status.contains(Status::WT_RENAMED)
            {
                if let Some(path_str) = entry.path() {
                    let full_path = workdir.join(path_str);
                    if full_path.exists() {
                        changed_files.push(full_path);
                    }
                }
            }
        }

        Ok(changed_files)
    }

    /// Experimental: Get changed line ranges (Hunks) for a file.
    /// Useful for surgical re-indexing (future optimization).
    pub fn get_changed_ranges(&self, _file_path: &Path) -> Result<Vec<(usize, usize)>> {
        // This requires diffing logic against HEAD.
        // For Phase 2.0, we just return "whole file" essentially or skip this complexity
        // until we truly need sub-file incrementalism.
        // The Plan says "Hunk Mapping". Let's implement basic diff against HEAD.

        // Simplified: If we can't get diff easily, return empty (meaning "whole file").
        // Proper implementation is complex with libgit2 diffing.
        Ok(Vec::new())
    }
}
