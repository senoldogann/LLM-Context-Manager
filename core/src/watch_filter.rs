//! MCP otomatik yenilemesinin dosya olaylarını süzen filtre. Manifest
//! taramasıyla aynı politikayı uygular; indeksin kendi yazdığı dosyalar ve
//! ignore kurallarına takılan build çıktıları yenileme tetiklemez. Git
//! olmayan projelerde `.gitignore` ve `.git/info/exclude` uygulanmaz, tarama
//! davranışıyla tutarlılık sağlanır.

use anyhow::Result;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::{Path, PathBuf};

/// Bir proje için önceden derlenmiş izleme kuralları.
pub struct WatchFilter {
    root: PathBuf,
    excluded: Vec<PathBuf>,
    ignore: Gitignore,
}

/// Git reposunun kökünü bulur: `.git` dizini veya dosyası olan yerin kanonik
/// yolunu, yoksa None.
fn find_git_root(path: &Path) -> Option<PathBuf> {
    let mut current = path.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Proje kökü ve indeks DB yolundan izleme filtresini kurar. Kök seviyesindeki
/// `.ignore`, `.ccmignore` her zaman okunur. `.gitignore` ve `.git/info/exclude`
/// yalnızca kök veya üst dizinleri git reposunun içindeyse okunur (walker ile
/// tutarlılık); iç içe ignore dosyaları kapsanmaz (kaçan olay yalnızca
/// değişiklik bulmayan bir yenileme maliyeti yaratır).
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
    let excluded =
        crate::with_canonical_variants(crate::index_artifact_paths(artifact_parent, db_path));

    let mut builder = GitignoreBuilder::new(&root);
    let in_git_repo = find_git_root(&root).is_some();

    // Taramanın öncelik sırasına göre ekle: düşük → yüksek, çünkü builder'da
    // son eşleşme kazanır.
    if in_git_repo {
        let git_exclude = root.join(".git/info/exclude");
        if git_exclude.is_file() {
            if let Some(error) = builder.add(&git_exclude) {
                return Err(anyhow::anyhow!(
                    "Git exclude file '{}' could not be parsed for watching: {}",
                    git_exclude.display(),
                    error
                ));
            }
        }
    }
    if in_git_repo {
        let gitignore = root.join(".gitignore");
        if gitignore.is_file() {
            if let Some(error) = builder.add(&gitignore) {
                return Err(anyhow::anyhow!(
                    "Gitignore file '{}' could not be parsed for watching: {}",
                    gitignore.display(),
                    error
                ));
            }
        }
    }
    let ignore_file = root.join(".ignore");
    if ignore_file.is_file() {
        if let Some(error) = builder.add(&ignore_file) {
            return Err(anyhow::anyhow!(
                "Ignore file '{}' could not be parsed for watching: {}",
                ignore_file.display(),
                error
            ));
        }
    }
    let ccmignore = root.join(".ccmignore");
    if ccmignore.is_file() {
        if let Some(error) = builder.add(&ccmignore) {
            return Err(anyhow::anyhow!(
                "CCM ignore file '{}' could not be parsed for watching: {}",
                ccmignore.display(),
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
        name == ".ccm" || name == ".agent" || crate::is_index_staging_dir_name(&name)
    });
    if tool_state || !crate::is_index_relevant_file(&filter.root, path) {
        return false;
    }
    !filter
        .ignore
        .matched_path_or_any_parents(relative, path.is_dir())
        .is_ignore()
}
