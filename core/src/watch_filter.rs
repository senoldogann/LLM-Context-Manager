//! MCP otomatik yenilemesinin dosya olaylarını süzen filtre. Manifest
//! taramasıyla aynı politikayı uygular; indeksin kendi yazdığı dosyalar (artefaktlar,
//! atomik yazımın geçici dosyaları, `ccm_learn` verisi) ve ignore kurallarına takılan
//! build çıktıları yenileme tetiklemez. Git olmayan projelerde `.gitignore` ve
//! `.git/info/exclude` uygulanmaz, tarama davranışıyla tutarlılık sağlanır; bu
//! yüzden indeksin kendi çıktısına karşı koruma `.git/info/exclude`'a değil bu
//! filtrenin kendi kurallarına dayanır.

use anyhow::Result;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::{Path, PathBuf};

/// Bir proje için önceden derlenmiş izleme kuralları.
pub struct WatchFilter {
    root: PathBuf,
    excluded: Vec<PathBuf>,
    /// İndeks artefaktlarının durduğu dizin (ham ve kanonik biçimleriyle);
    /// atomik yazımın geçici dosyaları yalnızca bu dizinin doğrudan altında
    /// artefakt sayılır.
    artifact_parents: Vec<PathBuf>,
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
    let artifact_parents = crate::with_canonical_variants(vec![artifact_parent.to_path_buf()]);

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
        artifact_parents,
        ignore,
    })
}

/// Yol, artefakt dizininin doğrudan altındaki bir atomik yazım geçici dosyası mı?
/// Artefakt dizini dışındaki aynı adlı dosyalar normal proje dosyasıdır.
fn is_artifact_temp_file(filter: &WatchFilter, path: &Path) -> bool {
    let Some(name) = path.file_name().map(|name| name.to_string_lossy()) else {
        return false;
    };
    crate::is_index_artifact_temp_name(&name)
        && path
            .parent()
            .is_some_and(|parent| filter.artifact_parents.iter().any(|dir| dir == parent))
}

/// Yol proje kökünün altında ve indeksin kendi yazdığı ya da araç durumuna ait
/// bir yol değilse köke göre göreli yolu döndürür. İndeksin kendi yazdığı yollar
/// (artefaktlar, artefakt dizinindeki geçici dosyalar, `ccm_learn` verisi)
/// ilgisizdir; aksi halde her yenileme kendi olayını okuyup ikinci bir tur çalıştırır.
fn project_relative_path<'a>(filter: &WatchFilter, path: &'a Path) -> Option<&'a Path> {
    let relative = path.strip_prefix(&filter.root).ok()?;
    if relative.as_os_str().is_empty() {
        return None;
    }
    if filter
        .excluded
        .iter()
        .any(|excluded| path.starts_with(excluded))
    {
        return None;
    }
    if is_artifact_temp_file(filter, path) {
        return None;
    }
    let tool_state = relative.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        name == ".ccm"
            || name == ".agent"
            || name == crate::LEARN_DIRECTORY
            || crate::is_index_staging_dir_name(&name)
    });
    (!tool_state).then_some(relative)
}

/// Olay yolunun indeksi değiştirebilecek bir proje dosyası olup olmadığını
/// bildirir. Silinmiş yollar için de çalışır (dosya içeriği okunmaz; yalnızca
/// dizin ayrımı için `is_dir` sorulur).
pub fn is_watch_relevant_path(filter: &WatchFilter, path: &Path) -> bool {
    let Some(relative) = project_relative_path(filter, path) else {
        return false;
    };
    if !crate::is_index_relevant_file(&filter.root, path) {
        return false;
    }
    !filter
        .ignore
        .matched_path_or_any_parents(relative, path.is_dir())
        .is_ignore()
}

/// Olay yolunun, tam taramanın indiği bir proje dizini olup olmadığını
/// bildirir. Dosya süzgecinden farkı: dosya uzantısı ve gizli dosya adı
/// kuralları dizinlere uygulanmaz (tarayıcı `assets.png/` gibi dizinlere de
/// iner); dışlanan dizin adları, araç durumu ve ignore kuralları aynen
/// uygulanır. Silinmiş ya da taşınmış dizinler için de çalışır.
pub fn is_watch_relevant_dir(filter: &WatchFilter, path: &Path) -> bool {
    let Some(relative) = project_relative_path(filter, path) else {
        return false;
    };
    crate::is_index_relevant_dir(relative)
        && !filter
            .ignore
            .matched_path_or_any_parents(relative, true)
            .is_ignore()
}
