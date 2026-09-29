//! MCP otomatik yenilemesinin dosya olaylarını süzen filtre. Manifest
//! taramasıyla aynı politikayı uygular; indeksin kendi yazdığı dosyalar ve
//! ignore kurallarına takılan build çıktıları yenileme tetiklemez.

use anyhow::Result;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::{Path, PathBuf};

/// Bir proje için önceden derlenmiş izleme kuralları.
pub struct WatchFilter {
    root: PathBuf,
    excluded: Vec<PathBuf>,
    ignore: Gitignore,
}

/// Proje kökü ve indeks DB yolundan izleme filtresini kurar. Kök seviyesindeki
/// `.gitignore`, `.ignore`, `.ccmignore` ve `.git/info/exclude` okunur; iç içe
/// ignore dosyaları kapsanmaz (kaçan olay yalnızca değişiklik bulmayan bir
/// yenileme maliyeti yaratır).
pub fn build_watch_filter(project_root: &Path, db_path: &Path) -> Result<WatchFilter> {
    let root = std::fs::canonicalize(project_root).map_err(|error| {
        anyhow::anyhow!(
            "Project root '{}' could not be resolved for watching: {}",
            project_root.display(),
            error
        )
    })?;
    let artifact_parent = db_path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "Invalid DB path '{}': cannot determine parent directory",
            db_path.display()
        )
    })?;
    let excluded = crate::index_artifact_paths(artifact_parent, db_path)
        .into_iter()
        .flat_map(|path| match std::fs::canonicalize(&path) {
            Ok(canonical) if canonical != path => vec![path, canonical],
            _ => vec![path],
        })
        .collect();

    let mut builder = GitignoreBuilder::new(&root);
    for candidate in [
        root.join(".gitignore"),
        root.join(".ignore"),
        root.join(".ccmignore"),
        root.join(".git/info/exclude"),
    ] {
        if !candidate.is_file() {
            continue;
        }
        if let Some(error) = builder.add(&candidate) {
            return Err(anyhow::anyhow!(
                "Ignore file '{}' could not be parsed for watching: {}",
                candidate.display(),
                error
            ));
        }
    }
    let ignore = builder.build().map_err(|error| {
        anyhow::anyhow!(
            "Watch ignore rules could not be built for '{}': {}",
            root.display(),
            error
        )
    })?;
    Ok(WatchFilter {
        root,
        excluded,
        ignore,
    })
}

/// Olay yolunun indeksi değiştirebilecek bir proje dosyası olup olmadığını
/// bildirir. Silinmiş yollar için de çalışır (dosya içeriği okunmaz; yalnızca
/// dizin ayrımı için `is_dir` sorulur).
pub fn is_watch_relevant_path(filter: &WatchFilter, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(&filter.root) else {
        return false;
    };
    if relative.as_os_str().is_empty() {
        return false;
    }
    if filter
        .excluded
        .iter()
        .any(|excluded| path.starts_with(excluded))
    {
        return false;
    }
    let tool_state = relative.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        name == ".ccm"
            || name == ".agent"
            || name == crate::GENERATIONS_DIRECTORY
            || name == crate::ACTIVATION_LOCK_DIRECTORY
            || name.starts_with(".ccm-rebuild-")
            || name.starts_with(".ccm-backup-")
    });
    if tool_state || !crate::is_index_relevant_file(&filter.root, path) {
        return false;
    }
    !filter
        .ignore
        .matched_path_or_any_parents(relative, path.is_dir())
        .is_ignore()
}
