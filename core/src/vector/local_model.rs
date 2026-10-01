//! Yerleşik embedding modelinin sabitlenmiş (pinned) dosyaları: model dizini,
//! SHA-256 doğrulaması ve Hugging Face'ten indirme.
//!
//! Dosyalar yalnızca sabitlenmiş revizyondan indirilir ve sabitlenmiş SHA-256
//! değerleriyle doğrulanır. Diskte önceden bulunan dosyalar (ağsız kurulum)
//! doğrulandıktan sonra olduğu gibi kullanılır. Uyuşmayan dosya açık bir
//! hatadır ve üzerine yazılmaz; indirilen içerik yalnızca doğrulandıktan sonra
//! yerine taşınır, yarım indirme hiçbir zaman nihai yolda kalmaz.
//!
//! Bu modül ONNX Runtime'a bağlı değildir; `ccm-cli models pull` yerel
//! sağlayıcının derlenmediği hedeflerde de dosyaları indirip doğrulayabilir.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;

/// Hugging Face deposundaki sabitlenmiş tek bir dosya.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinnedFile {
    /// Depo köküne göre yol (ör. `onnx/model_quint8_avx2.onnx`); model
    /// dizininde de aynı göreli yolda tutulur.
    pub path: &'static str,
    /// Dosya içeriğinin küçük harfli onaltılık SHA-256 özeti.
    pub sha256: &'static str,
    /// Bayt cinsinden boyut.
    pub size: u64,
}

/// Yerleşik embedding modelinin sabitlenmiş kaynağı ve çıkarım ayarları.
///
/// İndeks kimliği `repo` ve `revision` alanlarına dayanır: sabitlenmiş bir
/// dosya değişirse bu iki alandan biri de değişmelidir, aksi halde eski
/// vektörler yeni modelin vektörleriyle karışır.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalModelSpec {
    /// Hugging Face depo kimliği.
    pub repo: &'static str,
    /// Sabitlenmiş commit.
    pub revision: &'static str,
    /// Çıktı vektör boyutu.
    pub dim: usize,
    /// Girdiler bu token sayısında kesilir.
    pub max_tokens: usize,
    pub onnx: PinnedFile,
    pub tokenizer: PinnedFile,
    pub config: PinnedFile,
    pub special_tokens_map: PinnedFile,
    pub tokenizer_config: PinnedFile,
}

/// Varsayılan yerel model: IBM granite-embedding-97m-multilingual-r2
/// (Apache-2.0), IBM'in yayımladığı int8 (dinamik quantize) ONNX dosyası.
/// CLS pooling ve L2 normalizasyonu model kartıyla aynıdır; sorgu/doküman
/// öneki yoktur.
pub const DEFAULT_LOCAL_MODEL: LocalModelSpec = LocalModelSpec {
    repo: "ibm-granite/granite-embedding-97m-multilingual-r2",
    revision: "835ad14087e140460703cf0fae09f97d469d65c2",
    dim: 384,
    max_tokens: 512,
    onnx: PinnedFile {
        path: "onnx/model_quint8_avx2.onnx",
        sha256: "a6022dd8220ea6f6595562a1328ee216f4a94faa55362f2f4747c80f1e78772e",
        size: 98_247_878,
    },
    tokenizer: PinnedFile {
        path: "tokenizer.json",
        sha256: "4f2842d568e2724370aec203652a42ac783c7937f8347a1a2cc7506d71f1582f",
        size: 25_301_672,
    },
    config: PinnedFile {
        path: "config.json",
        sha256: "de948b0bdc6f356afad7a84b276d8dd7e7fe10fb9add1bb5e610621c28e41ebc",
        size: 1_216,
    },
    special_tokens_map: PinnedFile {
        path: "special_tokens_map.json",
        sha256: "013787ee251ff611722479197c00853b62113ad303cb0a36524231783c676c69",
        size: 871,
    },
    tokenizer_config: PinnedFile {
        path: "tokenizer_config.json",
        sha256: "6ed69389e30a8ecabfce2f9ebcdf0c908b34056f24d994340f2f216521c057d5",
        size: 12_860,
    },
};

impl LocalModelSpec {
    /// Modelin tüm sabitlenmiş dosyaları.
    pub fn files(&self) -> [PinnedFile; 5] {
        [
            self.onnx,
            self.tokenizer,
            self.config,
            self.special_tokens_map,
            self.tokenizer_config,
        ]
    }

    /// Tüm dosyaların toplam boyutu (bayt).
    pub fn total_bytes(&self) -> u64 {
        self.files().iter().map(|file| file.size).sum()
    }

    /// Model dosyalarının modeller köküne göre dizini: `<org>--<ad>/<revizyon>`.
    /// Revizyon yolda olduğu için sabitlenmiş sürüm değişince eski dosyalar
    /// çakışmaz, yeni sürüm kendi dizinine iner.
    pub fn relative_dir(&self) -> PathBuf {
        PathBuf::from(self.repo.replace('/', "--")).join(self.revision)
    }
}

/// Modellerin kök dizini: `CCM_MODEL_DIR` ya da `~/.ccm/models`.
pub fn models_root() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("CCM_MODEL_DIR").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|value| !value.is_empty())
        .context(
            "Neither HOME nor USERPROFILE is set; set CCM_MODEL_DIR to choose where the local embedding model is stored",
        )?;
    Ok(PathBuf::from(home).join(".ccm").join("models"))
}

/// Verilen modelin dosyalarının bulunduğu (ya da indirileceği) dizin.
pub fn model_dir(spec: &LocalModelSpec) -> Result<PathBuf> {
    Ok(models_root()?.join(spec.relative_dir()))
}

/// Dosyaların indirildiği kaynak: `HF_ENDPOINT` (ayna) ya da huggingface.co.
/// İçerik her durumda sabitlenmiş özetle doğrulanır.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSource {
    pub base_url: String,
    pub repo: String,
    pub revision: String,
}

impl ModelSource {
    /// Modelin sabitlenmiş revizyonu için ortamdaki uç noktayı kullanır.
    pub fn from_env(spec: &LocalModelSpec) -> Self {
        let base_url = std::env::var("HF_ENDPOINT")
            .ok()
            .map(|value| value.trim().trim_end_matches('/').to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "https://huggingface.co".to_string());
        Self {
            base_url,
            repo: spec.repo.to_string(),
            revision: spec.revision.to_string(),
        }
    }

    /// Dosyanın sabitlenmiş revizyondaki indirme adresi.
    pub fn url(&self, file: &PinnedFile) -> String {
        format!(
            "{}/{}/resolve/{}/{}",
            self.base_url, self.repo, self.revision, file.path
        )
    }
}

/// Bir model dosyasının diskteki durumu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileState {
    Missing,
    Verified,
    /// Dosya var ama sabitlenmiş boyut/özetle uyuşmuyor.
    Mismatch {
        detail: String,
    },
}

/// `ensure` sonrası bir dosyanın nasıl hazırlandığı.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOutcome {
    AlreadyPresent,
    Downloaded,
}

/// Diskteki ya da indirilen bir model dosyası sabitlenmiş özetle uyuşmuyor.
/// Bütünlük hatasıdır: dosya kullanılmaz ve üzerine yazılmaz.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFileMismatch {
    pub path: PathBuf,
    pub detail: String,
    pub url: String,
}

impl std::fmt::Display for ModelFileMismatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "local embedding model file '{}' does not match its pinned checksum ({}). Delete it and run `ccm-cli models pull`, or replace it with the file from {}",
            self.path.display(),
            self.detail,
            self.url
        )
    }
}

impl std::error::Error for ModelFileMismatch {}

/// Model dosyası indirilemedi (ağ hatası, zaman aşımı, HTTP durumu). İndeksleyici
/// bunu embedding servisinin erişilemez olmasıyla aynı sayar: graf aktive
/// edilir, semantik katman sonraki indekslemede tamamlanır.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelDownloadFailed {
    pub url: String,
    pub detail: String,
    pub directory: PathBuf,
}

impl std::fmt::Display for ModelDownloadFailed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "the local embedding model could not be downloaded from {} ({}). Check network or proxy access to huggingface.co (HF_ENDPOINT selects a mirror), or copy the model files into {} and run `ccm-cli models pull` to verify them",
            self.url,
            self.detail,
            self.directory.display()
        )
    }
}

impl std::error::Error for ModelDownloadFailed {}

/// İndirme denemesi sayısı; ağ hataları ve 5xx/429 yanıtları yeniden denenir.
const DOWNLOAD_ATTEMPTS: u32 = 3;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Veri akışı bu süre boyunca durursa deneme başarısız sayılır.
const READ_TIMEOUT: Duration = Duration::from_secs(60);
/// İlerleme bu kadar baytta bir log'lanır.
const PROGRESS_STEP_BYTES: u64 = 16 * 1024 * 1024;

/// Dosyanın diskteki durumunu okur; varsa boyutunu ve özetini denetler.
pub fn inspect_file(dir: &Path, file: &PinnedFile) -> Result<FileState> {
    let path = dir.join(file.path);
    let metadata = match std::fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(FileState::Missing)
        }
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "Local embedding model file '{}' could not be inspected",
                    path.display()
                )
            })
        }
    };
    if metadata.len() != file.size {
        return Ok(FileState::Mismatch {
            detail: format!("size {} bytes, expected {}", metadata.len(), file.size),
        });
    }
    let actual = sha256_of_file(&path)?;
    if actual == file.sha256 {
        Ok(FileState::Verified)
    } else {
        Ok(FileState::Mismatch {
            detail: format!("sha256 {}, expected {}", actual, file.sha256),
        })
    }
}

/// Modelin tüm dosyalarının durumunu döndürür (ağ kullanılmaz).
pub fn inspect_model(spec: &LocalModelSpec, dir: &Path) -> Result<Vec<(PinnedFile, FileState)>> {
    spec.files()
        .into_iter()
        .map(|file| inspect_file(dir, &file).map(|state| (file, state)))
        .collect()
}

/// Dosyayı belleğe okur ve sabitlenmiş boyut/özetle doğrular; model yüklenirken
/// her dosya tek okumayla hem doğrulanır hem kullanılır.
pub fn read_verified(dir: &Path, file: &PinnedFile, source: &ModelSource) -> Result<Vec<u8>> {
    let path = dir.join(file.path);
    let bytes = std::fs::read(&path).with_context(|| {
        format!(
            "Local embedding model file '{}' could not be read",
            path.display()
        )
    })?;
    let mismatch = |detail: String| ModelFileMismatch {
        path: path.clone(),
        detail,
        url: source.url(file),
    };
    if bytes.len() as u64 != file.size {
        return Err(mismatch(format!(
            "size {} bytes, expected {}",
            bytes.len(),
            file.size
        ))
        .into());
    }
    let actual = hex::encode(Sha256::digest(&bytes));
    if actual != file.sha256 {
        return Err(mismatch(format!("sha256 {}, expected {}", actual, file.sha256)).into());
    }
    Ok(bytes)
}

/// Hedef dosyaya ait, güvenli yaştan daha eski yarım indirmeleri temizler.
/// Yeni `.part` dosyalarına dokunmaz; böylece eşzamanlı indiricilerle yarışmaz.
fn remove_stale_partial_downloads(target: &Path, stale_after: Duration) {
    let (Some(parent), Some(name)) = (target.parent(), target.file_name()) else {
        return;
    };
    let prefix = format!("{}.", name.to_string_lossy());
    let entries = match std::fs::read_dir(parent) {
        Ok(entries) => entries,
        // İlk indirmede dizin henüz yoktur; temizlenecek bir şey yok.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            tracing::warn!(
                path = %parent.display(),
                error = %error,
                "Failed to list partial model downloads for cleanup"
            );
            return;
        }
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().to_string();
        if !file_name.starts_with(&prefix) || !file_name.ends_with(".part") {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age >= stale_after);
        if stale {
            if let Err(error) = std::fs::remove_file(entry.path()) {
                tracing::warn!(
                    path = %entry.path().display(),
                    error = %error,
                    "Failed to remove stale partial model download"
                );
            }
        }
    }
}

/// Eksik dosyaları sabitlenmiş kaynaktan indirir; mevcut dosyalara dokunmaz
/// (onları okuyan taraf doğrular). İndirilen içerik doğrulanmadan yerine
/// taşınmaz.
pub async fn download_missing_files(
    files: &[PinnedFile],
    dir: &Path,
    source: &ModelSource,
) -> Result<Vec<(PinnedFile, FileOutcome)>> {
    let mut outcomes = Vec::with_capacity(files.len());
    let mut client: Option<reqwest::Client> = None;
    for file in files {
        let target = dir.join(file.path);
        let exists = tokio::fs::try_exists(&target).await.with_context(|| {
            format!(
                "Local embedding model file '{}' could not be inspected",
                target.display()
            )
        })?;
        if exists {
            outcomes.push((*file, FileOutcome::AlreadyPresent));
            continue;
        }
        remove_stale_partial_downloads(&target, Duration::from_secs(24 * 60 * 60));
        let client = match client.as_ref() {
            Some(client) => client,
            None => client.insert(download_client()?),
        };
        download_file(client, file, dir, source).await?;
        outcomes.push((*file, FileOutcome::Downloaded));
    }
    Ok(outcomes)
}

/// Eksikleri indirir, ardından tüm dosyaları doğrular. `ccm-cli models pull`
/// ve doktor çıktısı için; uyuşmayan dosya `ModelFileMismatch` hatasıdır.
pub async fn pull_model_files(
    files: &[PinnedFile],
    dir: &Path,
    source: &ModelSource,
) -> Result<Vec<(PinnedFile, FileOutcome)>> {
    let outcomes = download_missing_files(files, dir, source).await?;
    let verify_dir = dir.to_path_buf();
    let verify_files = files.to_vec();
    let verify_source = source.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        for file in &verify_files {
            match inspect_file(&verify_dir, file)? {
                FileState::Verified => {}
                FileState::Missing => anyhow::bail!(
                    "Local embedding model file '{}' disappeared after download",
                    verify_dir.join(file.path).display()
                ),
                FileState::Mismatch { detail } => {
                    return Err(ModelFileMismatch {
                        path: verify_dir.join(file.path),
                        detail,
                        url: verify_source.url(file),
                    }
                    .into())
                }
            }
        }
        Ok(())
    })
    .await
    .map_err(|error| anyhow::anyhow!("Model verification task failed: {}", error))??;
    Ok(outcomes)
}

/// İndirme istemcisi: toplam süre sınırı yok (yavaş bağlantıda 100 MB uzun
/// sürebilir), bağlantı ve veri akışı ayrı ayrı zaman aşımına tabi.
fn download_client() -> Result<reqwest::Client> {
    crate::vector::remote::build_http_client(&|builder: reqwest::ClientBuilder| {
        builder
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(READ_TIMEOUT)
            .user_agent(concat!("ccm/", env!("CARGO_PKG_VERSION")))
    })
}

/// Tek bir indirme denemesinin hatası.
enum AttemptError {
    /// Ağ hatası, zaman aşımı ya da 5xx/429: yeniden denenir.
    Retryable(String),
    /// Yeniden denemenin fayda etmeyeceği HTTP durumu (ör. 404).
    Unavailable(String),
    /// Bütünlük ya da yerel dosya sistemi hatası: hemen yükseltilir.
    Fatal(anyhow::Error),
}

/// Dosyayı yeniden denemelerle indirir; her başarısız deneme uyarı olarak
/// log'lanır, son hata `ModelDownloadFailed` olarak yükseltilir.
async fn download_file(
    client: &reqwest::Client,
    file: &PinnedFile,
    dir: &Path,
    source: &ModelSource,
) -> Result<()> {
    let url = source.url(file);
    let target = dir.join(file.path);
    let parent = target.parent().unwrap_or(dir).to_path_buf();
    tokio::fs::create_dir_all(&parent).await.with_context(|| {
        format!(
            "Local embedding model directory '{}' could not be created",
            parent.display()
        )
    })?;
    tracing::info!(
        file = file.path,
        size_bytes = file.size,
        url = %url,
        "Downloading local embedding model file"
    );
    let unavailable = |detail: String| ModelDownloadFailed {
        url: url.clone(),
        detail,
        directory: dir.to_path_buf(),
    };
    let mut attempt = 1;
    loop {
        match download_attempt(client, &url, file, &target).await {
            Ok(()) => return Ok(()),
            Err(AttemptError::Fatal(error)) => return Err(error),
            Err(AttemptError::Unavailable(detail)) => return Err(unavailable(detail).into()),
            Err(AttemptError::Retryable(detail)) if attempt < DOWNLOAD_ATTEMPTS => {
                let delay = Duration::from_secs(u64::from(attempt));
                tracing::warn!(
                    file = file.path,
                    url = %url,
                    attempt,
                    retry_in_secs = delay.as_secs(),
                    error = %detail,
                    "Local embedding model download failed; retrying"
                );
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
            Err(AttemptError::Retryable(detail)) => {
                return Err(unavailable(format!("{} attempt(s): {}", attempt, detail)).into())
            }
        }
    }
}

/// Tek indirme denemesi: içerik geçici dosyaya akarken özetlenir; boyut ve
/// özet tutarsa dosya atomik olarak yerine taşınır, aksi halde silinir.
async fn download_attempt(
    client: &reqwest::Client,
    url: &str,
    file: &PinnedFile,
    target: &Path,
) -> std::result::Result<(), AttemptError> {
    let started = Instant::now();
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|error| AttemptError::Retryable(format!("{:#}", anyhow::Error::new(error))))?;
    let status = response.status();
    if !status.is_success() {
        let detail = format!("HTTP {}", status);
        return Err(
            if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                AttemptError::Retryable(detail)
            } else {
                AttemptError::Unavailable(detail)
            },
        );
    }

    let temp = temporary_path(target);
    let streamed = stream_to_file(&mut response, url, file, (&temp, target)).await;
    let result = match streamed {
        Ok(()) => crate::replace_file_atomically(&temp, target).map_err(|error| {
            AttemptError::Fatal(anyhow::anyhow!(
                "Downloaded model file could not be moved to '{}': {}",
                target.display(),
                error
            ))
        }),
        Err(error) => Err(error),
    };
    if result.is_err() {
        if let Err(error) = std::fs::remove_file(&temp) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(path = %temp.display(), error = %error, "Partial model download could not be removed");
            }
        }
        return result;
    }
    tracing::info!(
        file = file.path,
        size_bytes = file.size,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Downloaded and verified local embedding model file"
    );
    Ok(())
}

/// Yanıt gövdesini geçici dosyaya yazar ve sabitlenmiş boyut/özetle doğrular;
/// hata mesajları dosyanın nihai yolunu gösterir.
async fn stream_to_file(
    response: &mut reqwest::Response,
    url: &str,
    file: &PinnedFile,
    (temp, target): (&Path, &Path),
) -> std::result::Result<(), AttemptError> {
    let fatal_io = |error: std::io::Error| {
        AttemptError::Fatal(anyhow::anyhow!(
            "Model download could not be written to '{}': {}",
            temp.display(),
            error
        ))
    };
    let mut output = tokio::fs::File::create(temp).await.map_err(fatal_io)?;
    let mut hasher = Sha256::new();
    let mut written: u64 = 0;
    let mut next_progress = PROGRESS_STEP_BYTES;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| AttemptError::Retryable(format!("{:#}", anyhow::Error::new(error))))?
    {
        written += chunk.len() as u64;
        // Sabitlenmiş boyutu aşan gövde yanlış içeriktir; sınırsız indirme yapılmaz.
        if written > file.size {
            return Err(AttemptError::Fatal(
                ModelFileMismatch {
                    path: target.to_path_buf(),
                    detail: format!("download exceeded the pinned size of {} bytes", file.size),
                    url: url.to_string(),
                }
                .into(),
            ));
        }
        hasher.update(&chunk);
        output.write_all(&chunk).await.map_err(fatal_io)?;
        if written >= next_progress {
            tracing::info!(
                file = file.path,
                downloaded_bytes = written,
                size_bytes = file.size,
                "Local embedding model download progress"
            );
            next_progress += PROGRESS_STEP_BYTES;
        }
    }
    output.flush().await.map_err(fatal_io)?;
    output.sync_all().await.map_err(fatal_io)?;
    drop(output);

    if written != file.size {
        // Bağlantı gövde bitmeden kapandı: geçici bir ağ hatasıdır.
        return Err(AttemptError::Retryable(format!(
            "download ended after {} of {} bytes",
            written, file.size
        )));
    }
    let actual = hex::encode(hasher.finalize());
    if actual != file.sha256 {
        return Err(AttemptError::Fatal(
            ModelFileMismatch {
                path: target.to_path_buf(),
                detail: format!(
                    "downloaded content has sha256 {}, expected {}",
                    actual, file.sha256
                ),
                url: url.to_string(),
            }
            .into(),
        ));
    }
    Ok(())
}

/// Aynı dizine eşzamanlı indiren süreçler birbirinin geçici dosyasını ezmesin
/// diye süreç ve zaman damgalı geçici yol.
fn temporary_path(target: &Path) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "model".to_string());
    target.with_file_name(format!("{}.{}.{}.part", name, std::process::id(), nanos))
}

/// Dosyanın SHA-256 özeti; büyük dosyalar parça parça okunur.
fn sha256_of_file(path: &Path) -> Result<String> {
    let mut reader = std::fs::File::open(path).with_context(|| {
        format!(
            "Local embedding model file '{}' could not be opened",
            path.display()
        )
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = reader.read(&mut buffer).with_context(|| {
            format!(
                "Local embedding model file '{}' could not be read",
                path.display()
            )
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::{
        download_missing_files, inspect_file, pull_model_files, remove_stale_partial_downloads,
        FileOutcome, FileState, ModelDownloadFailed, ModelFileMismatch, ModelSource, PinnedFile,
        DEFAULT_LOCAL_MODEL,
    };
    use sha2::{Digest, Sha256};
    use std::io::{BufRead, BufReader, Write};
    use std::time::Duration;

    const CONTENT: &[u8] = b"pinned model bytes";

    fn pinned(path: &'static str, content: &[u8]) -> PinnedFile {
        let sha256: &'static str = Box::leak(hex::encode(Sha256::digest(content)).into_boxed_str());
        PinnedFile {
            path,
            sha256,
            size: content.len() as u64,
        }
    }

    /// Her isteğe sabit gövdeyle yanıt veren yerel HTTP sunucusu; istek
    /// sayısını ve istenen yolları kaydeder.
    fn serve(
        status: &'static str,
        body: &'static [u8],
    ) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = format!("http://{}", listener.local_addr().expect("local addr"));
        let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = requests.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let Ok(read_half) = stream.try_clone() else {
                    continue;
                };
                let mut reader = BufReader::new(read_half);
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
                    continue;
                }
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                }
                seen.lock().expect("request log").push(
                    request_line
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("")
                        .to_string(),
                );
                let head = format!(
                    "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    status,
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body);
            }
        });
        (address, requests)
    }

    fn source(base_url: String) -> ModelSource {
        ModelSource {
            base_url,
            repo: "org/model".to_string(),
            revision: "rev1".to_string(),
        }
    }

    #[test]
    fn default_model_is_pinned_to_a_revision_and_checksums() {
        assert_eq!(DEFAULT_LOCAL_MODEL.revision.len(), 40);
        for file in DEFAULT_LOCAL_MODEL.files() {
            assert_eq!(file.sha256.len(), 64, "{} must pin a sha256", file.path);
            assert!(file.size > 0);
        }
        assert_eq!(
            DEFAULT_LOCAL_MODEL.relative_dir(),
            std::path::Path::new("ibm-granite--granite-embedding-97m-multilingual-r2")
                .join("835ad14087e140460703cf0fae09f97d469d65c2")
        );
    }

    #[test]
    fn stale_partial_cleanup_only_removes_matching_parts() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let parent = dir.path().join("onnx");
        std::fs::create_dir_all(&parent)?;
        let target = parent.join("model.onnx");
        let matching = parent.join("model.onnx.123.456.part");
        let unrelated = parent.join("other.onnx.123.456.part");
        std::fs::write(&matching, b"partial")?;
        std::fs::write(&unrelated, b"keep")?;

        remove_stale_partial_downloads(&target, Duration::ZERO);

        assert!(!matching.exists());
        assert!(unrelated.exists());
        Ok(())
    }

    #[tokio::test]
    async fn missing_file_is_downloaded_verified_and_moved_into_place() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = pinned("onnx/model.onnx", CONTENT);
        let (base_url, requests) = serve("200 OK", CONTENT);

        let outcomes = pull_model_files(&[file], dir.path(), &source(base_url)).await?;

        assert_eq!(outcomes, vec![(file, FileOutcome::Downloaded)]);
        assert_eq!(std::fs::read(dir.path().join("onnx/model.onnx"))?, CONTENT);
        assert_eq!(
            requests.lock().expect("request log").as_slice(),
            ["/org/model/resolve/rev1/onnx/model.onnx"]
        );
        assert!(
            std::fs::read_dir(dir.path().join("onnx"))?.count() == 1,
            "no partial download may remain"
        );
        Ok(())
    }

    #[tokio::test]
    async fn verified_file_already_on_disk_is_used_without_network() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = pinned("tokenizer.json", CONTENT);
        std::fs::write(dir.path().join("tokenizer.json"), CONTENT)?;
        // Kapalı port: istek atılsaydı bağlantı hatası verirdi.
        let outcomes = pull_model_files(
            &[file],
            dir.path(),
            &source("http://127.0.0.1:9".to_string()),
        )
        .await?;
        assert_eq!(outcomes, vec![(file, FileOutcome::AlreadyPresent)]);
        Ok(())
    }

    #[tokio::test]
    async fn tampered_download_is_rejected_and_never_lands_on_disk() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = pinned("model.onnx", CONTENT);
        let (base_url, _) = serve("200 OK", b"tampered model byte");

        let error = download_missing_files(&[file], dir.path(), &source(base_url))
            .await
            .expect_err("a checksum mismatch must fail the download");

        assert!(error.is::<ModelFileMismatch>(), "{error:#}");
        assert_eq!(
            std::fs::read_dir(dir.path())?.count(),
            0,
            "neither the file nor a partial download may remain"
        );
        Ok(())
    }

    #[tokio::test]
    async fn pre_placed_file_with_wrong_content_is_an_explicit_error() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = pinned("config.json", CONTENT);
        std::fs::write(dir.path().join("config.json"), b"edited model bytes")?;

        assert!(matches!(
            inspect_file(dir.path(), &file)?,
            FileState::Mismatch { .. }
        ));
        let error = pull_model_files(&[file], dir.path(), &source("http://127.0.0.1:9".into()))
            .await
            .expect_err("a mismatching pre-placed file must not be accepted");
        assert!(error.is::<ModelFileMismatch>(), "{error:#}");
        assert_eq!(
            std::fs::read(dir.path().join("config.json"))?,
            b"edited model bytes",
            "the user's file must not be overwritten"
        );
        Ok(())
    }

    #[tokio::test]
    async fn missing_remote_file_reports_the_download_as_unavailable() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = pinned("model.onnx", CONTENT);
        let (base_url, requests) = serve("404 Not Found", b"");

        let error = download_missing_files(&[file], dir.path(), &source(base_url))
            .await
            .expect_err("HTTP 404 must fail the download");

        assert!(error.is::<ModelDownloadFailed>(), "{error:#}");
        assert_eq!(
            requests.lock().expect("request log").len(),
            1,
            "a 404 is not retried"
        );
        Ok(())
    }
}
