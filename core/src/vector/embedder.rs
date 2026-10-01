//! Embedding sağlayıcısının seçimi, indeksteki vektörlerin kimliği ve
//! yapılandırılmış embedder'ın çalıştırılması.
//!
//! Sağlayıcı seçimi:
//! - `EMBEDDING_PROVIDER=local|ollama|openai` (ve OpenAI-uyumlu adlar) açıkça
//!   verilmişse o kullanılır.
//! - Verilmemişse ve `EMBEDDING_HOST` yoksa `~/.ccm/.env`'deki
//!   `OPENAI_API_KEY` OpenAI'ı seçer (`EMBEDDING_MODEL` yalnızca modeli
//!   belirler); yalnızca kabukta export edilmiş bir anahtar seçmez.
//! - Bu anahtar yokken `EMBEDDING_HOST` ya da `EMBEDDING_MODEL` ayarlıysa
//!   önceki Ollama/OpenAI çözümü korunur. Aksi halde desteklenen hedeflerde
//!   yerleşik yerel model kullanılır. Yerel sağlayıcının derlenmediği hedefte
//!   (x86_64-apple-darwin) sağlayıcı yapılandırılmamış sayılır: embedding
//!   atlanır, graf-yalnız indeks kurulur ve nedeni kullanıcıya bildirilir.
//! - `CCM_EMBEDDING_FIXTURE` ve `CCM_DISABLE_EMBEDDER` seçimden önce gelir.

use crate::vector::local_model::{LocalModelSpec, DEFAULT_LOCAL_MODEL};
use crate::vector::remote::{
    load_user_env_file, provider_label, resolve_provider, user_env_file_defines_openai_key,
    Provider, RemoteEmbedder, DEFAULT_OLLAMA_HOST, DEFAULT_OPENAI_HOST, DEFAULT_OPENAI_MODEL,
    DEFAULT_REMOTE_MODEL,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Bu derleme hedefinde yerleşik yerel embedder var mı? ONNX Runtime'ın hazır
/// ikilileri x86_64-apple-darwin için yayımlanmadığından orada derlenmez.
pub const LOCAL_EMBEDDER_AVAILABLE: bool =
    cfg!(not(all(target_os = "macos", target_arch = "x86_64")));

/// `CCM_DISABLE_EMBEDDER` (ya da `EMBEDDING_DISABLED`) açık mı?
pub fn embedder_disabled_by_env() -> bool {
    std::env::var("CCM_DISABLE_EMBEDDER")
        .or_else(|_| std::env::var("EMBEDDING_DISABLED"))
        .map(|value| matches!(value.to_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

/// `CCM_EMBED_BATCH_SIZE` verilmediğinde uzak sağlayıcıya tek HTTP isteğinde
/// gönderilen metin sayısı.
const DEFAULT_REMOTE_BATCH_SIZE: usize = 32;

/// Pozitif tam sayı alan ortam değişkeni. Tanımsız ya da boşsa `None`; pozitif
/// tam sayı olmayan değer açık bir hatadır.
pub(crate) fn positive_count_from_env(name: &str) -> Result<Option<usize>> {
    match std::env::var(name) {
        Ok(value) if value.trim().is_empty() => Ok(None),
        Ok(value) => value
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|count| *count > 0)
            .map(Some)
            .ok_or_else(|| anyhow::anyhow!("{} must be a positive integer, got '{}'", name, value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(anyhow::anyhow!("{} is not valid: {}", name, error)),
    }
}

/// `CCM_EMBED_BATCH_SIZE`: uzak sağlayıcıya (Ollama/OpenAI) tek HTTP isteğinde
/// gönderilen en fazla metin sayısı. Yerel modeli etkilemez; onun çıkarım
/// batch'i `CCM_LOCAL_EMBED_BATCH`'tir (bkz. `vector::local`).
pub fn remote_batch_size_from_env() -> Result<usize> {
    Ok(positive_count_from_env("CCM_EMBED_BATCH_SIZE")?.unwrap_or(DEFAULT_REMOTE_BATCH_SIZE))
}

/// `CCM_EMBEDDING_FIXTURE` ile verilen fixture yolu (boş değer tanımsız sayılır).
pub fn fixture_path_from_env() -> Option<PathBuf> {
    std::env::var("CCM_EMBEDDING_FIXTURE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
}

/// Sağlayıcı seçiminin girdileri; boş değerler tanımsız sayılır.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderSettings {
    /// `EMBEDDING_PROVIDER`
    pub provider: Option<String>,
    /// `EMBEDDING_HOST`
    pub host: Option<String>,
    /// `EMBEDDING_MODEL`
    pub model: Option<String>,
    /// `~/.ccm/.env` boş olmayan bir `OPENAI_API_KEY` tanımlıyor mu? Kabukta
    /// export edilmiş anahtar sayılmaz; anahtarın kendisi bu yapıda tutulmaz.
    pub openai_key_in_user_env_file: bool,
}

impl ProviderSettings {
    /// Değerleri süreç ortamından okur (`~/.ccm/.env` önceden yüklenmiş olmalı);
    /// OpenAI anahtarının varlığını yalnızca `~/.ccm/.env` dosyasından okur.
    pub fn from_env() -> Result<Self> {
        let read = |name: &str| {
            std::env::var(name)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };
        Ok(Self {
            provider: read("EMBEDDING_PROVIDER"),
            host: read("EMBEDDING_HOST"),
            model: read("EMBEDDING_MODEL"),
            openai_key_in_user_env_file: user_env_file_defines_openai_key()?,
        })
    }

    /// Sağlayıcı çözümlemesi için eski varsayılan adres. Açık host yokken
    /// provider adı yine seçimi belirler.
    fn resolution_host(&self) -> String {
        self.host
            .clone()
            .unwrap_or_else(|| DEFAULT_OLLAMA_HOST.to_string())
    }

    /// Resmi OpenAI varsayılanlarını yalnızca gerçek OpenAI seçimi için kullanır.
    /// OpenAI-uyumlu özel sağlayıcıların eski host/model varsayılanlarını değiştirmez.
    fn uses_official_openai_defaults(&self, provider: &Provider) -> bool {
        if *provider != Provider::OpenAI {
            return false;
        }
        if let Some(host) = self.host.as_deref() {
            let normalized = host.trim_end_matches('/');
            return normalized == DEFAULT_OPENAI_HOST
                || normalized.strip_prefix(DEFAULT_OPENAI_HOST) == Some("/embeddings");
        }
        self.provider
            .as_deref()
            .map(|name| name.eq_ignore_ascii_case("openai"))
            .unwrap_or(true)
    }

    /// Seçilmiş uzak sağlayıcının varsayılan adresi.
    pub fn host_or_default_for(&self, provider: &Provider) -> String {
        self.host.clone().unwrap_or_else(|| {
            if self.uses_official_openai_defaults(provider) {
                DEFAULT_OPENAI_HOST.to_string()
            } else {
                DEFAULT_OLLAMA_HOST.to_string()
            }
        })
    }

    /// Seçilmiş uzak sağlayıcının varsayılan modeli.
    pub fn model_or_default_for(&self, provider: &Provider) -> String {
        self.model.clone().unwrap_or_else(|| {
            if self.uses_official_openai_defaults(provider) {
                DEFAULT_OPENAI_MODEL.to_string()
            } else {
                DEFAULT_REMOTE_MODEL.to_string()
            }
        })
    }
}

/// Seçilen embedding sağlayıcısı.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderChoice {
    /// Süreç içinde çalışan yerleşik ONNX modeli.
    Local,
    /// Ollama ya da OpenAI-uyumlu HTTP servisi.
    Remote(Provider),
    /// Yerel modelin derlenmediği hedefte hiçbir sağlayıcı yapılandırılmamış:
    /// semantik arama kapalıdır (bkz. `EMBEDDER_UNCONFIGURED_REASON`).
    Unconfigured,
}

/// `ProviderChoice::Unconfigured` durumunda kullanıcıya gösterilen neden ve
/// semantik aramanın nasıl açılacağı.
pub const EMBEDDER_UNCONFIGURED_REASON: &str = "no embedding provider is configured and the built-in local model is not available on x86_64-apple-darwin (no prebuilt ONNX Runtime); add OPENAI_API_KEY to ~/.ccm/.env or set EMBEDDING_PROVIDER=openai|ollama";

/// Yerel sağlayıcı bu derleme hedefinde yok, ama açıkça istendi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalEmbedderUnsupported;

impl std::fmt::Display for LocalEmbedderUnsupported {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(
            "EMBEDDING_PROVIDER=local is not available on this platform (x86_64-apple-darwin has no prebuilt ONNX Runtime); use EMBEDDING_PROVIDER=ollama or openai",
        )
    }
}

impl std::error::Error for LocalEmbedderUnsupported {}

/// Sağlayıcıyı seçer (bkz. modül belgesi). Saf fonksiyondur; ortam okumaz.
pub fn choose_provider(
    settings: &ProviderSettings,
    local_available: bool,
) -> Result<ProviderChoice> {
    let host = settings.resolution_host();
    match settings.provider.as_deref().map(str::to_ascii_lowercase) {
        Some(name) if name == "local" => {
            if !local_available {
                return Err(LocalEmbedderUnsupported.into());
            }
            if let Some(model) = settings
                .model
                .as_deref()
                .filter(|model| *model != DEFAULT_LOCAL_MODEL.repo)
            {
                anyhow::bail!(
                    "EMBEDDING_MODEL='{}' cannot be used with EMBEDDING_PROVIDER=local: the built-in model is pinned to {}. Unset EMBEDDING_MODEL, or set EMBEDDING_PROVIDER=ollama|openai to use another model",
                    model,
                    DEFAULT_LOCAL_MODEL.repo
                );
            }
            Ok(ProviderChoice::Local)
        }
        Some(name) => Ok(ProviderChoice::Remote(resolve_provider(&name, &host))),
        // Dosyadaki anahtar, açık host yokken yalnızca model verilmiş olsa da
        // OpenAI'ı seçer: `EMBEDDING_MODEL=text-embedding-3-large` OpenAI'ın
        // modelini değiştirir, eski kurala göre Ollama'ya düşmez.
        None if settings.host.is_none() && settings.openai_key_in_user_env_file => {
            Ok(ProviderChoice::Remote(Provider::OpenAI))
        }
        None if settings.host.is_some() || settings.model.is_some() => {
            Ok(ProviderChoice::Remote(resolve_provider("", &host)))
        }
        None if local_available => Ok(ProviderChoice::Local),
        None => Ok(ProviderChoice::Unconfigured),
    }
}

/// Ortamdaki yapılandırmadan sağlayıcıyı seçer; önce `~/.ccm/.env` yüklenir.
pub fn configured_provider() -> Result<(ProviderChoice, ProviderSettings)> {
    load_user_env_file()?;
    let settings = ProviderSettings::from_env()?;
    let choice = choose_provider(&settings, LOCAL_EMBEDDER_AVAILABLE)?;
    Ok((choice, settings))
}

/// Sağlayıcı yapılandırılmamış mı (bkz. `ProviderChoice::Unconfigured`)? Bu
/// durumda embedding atlanır ve graf-yalnız indeks kurulur. Fixture ve
/// kapatma bayrağı seçimden önce gelir. Yapılandırma hatası kararı
/// değiştirmez: aynı hata embedder kurulurken açıkça yüzeye çıkar ve graf
/// araçlarını kullanılamaz kılmaz.
pub fn embedder_unconfigured() -> bool {
    // Önce `configured_provider` çalışır: bayraklar `~/.ccm/.env`'de de olabilir.
    let unconfigured = matches!(configured_provider(), Ok((ProviderChoice::Unconfigured, _)))
        && fixture_path_from_env().is_none()
        && !embedder_disabled_by_env();
    if unconfigured {
        static NOTICE: std::sync::Once = std::sync::Once::new();
        NOTICE.call_once(|| {
            tracing::warn!(
                reason = EMBEDDER_UNCONFIGURED_REASON,
                "Semantic search is off; graph tools keep working"
            );
        });
    }
    unconfigured
}

/// İndeksteki vektörleri üreten embedding kaynağının kimliği; indeks
/// manifestinde saklanır.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingIdentity {
    /// `local`, `ollama`, `openai` ya da `fixture`.
    pub provider: String,
    pub model: String,
    /// Sabitlenmiş model revizyonu (yalnızca yerel model).
    #[serde(default)]
    pub revision: Option<String>,
    pub dim: usize,
}

impl EmbeddingIdentity {
    /// Yerleşik yerel modelin kimliği.
    pub fn local(spec: &LocalModelSpec) -> Self {
        Self {
            provider: "local".to_string(),
            model: spec.repo.to_string(),
            revision: Some(spec.revision.to_string()),
            dim: spec.dim,
        }
    }
}

impl std::fmt::Display for EmbeddingIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{} {}", self.provider, self.model)?;
        if let Some(revision) = &self.revision {
            write!(formatter, "@{}", &revision[..revision.len().min(12)])?;
        }
        write!(formatter, " ({}-d)", self.dim)
    }
}

/// Yapılandırılmış embedding kaynağı; model yüklenmeden ve ağ çağrısı
/// yapılmadan çözülür.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddingSource {
    /// Embedding kapalı (`CCM_DISABLE_EMBEDDER` ya da yapılandırılmamış
    /// sağlayıcı): vektör üretilmez, kimlik denetlenmez.
    Disabled,
    /// Deterministik NDJSON fixture'ı; kimlik meta satırından gelir.
    Fixture(EmbeddingIdentity),
    /// Yerleşik yerel model.
    Local,
    /// Uzak servis: vektör boyutu ilk vektörle öğrenilir.
    Remote { provider: Provider, model: String },
}

impl EmbeddingSource {
    /// Ortamdan çözer: fixture, ardından kapatma bayrağı, ardından sağlayıcı
    /// seçimi. Bayraklar okunmadan önce `~/.ccm/.env` yüklenir; oradaki
    /// `CCM_DISABLE_EMBEDDER` ya da fixture ayarı da geçerlidir.
    pub fn from_env() -> Result<Self> {
        load_user_env_file()?;
        if let Some(path) = fixture_path_from_env() {
            let fixture = crate::vector::store::load_fixture_cached(&path)?;
            return Ok(Self::Fixture(fixture.identity()));
        }
        if embedder_disabled_by_env() {
            return Ok(Self::Disabled);
        }
        let (choice, settings) = configured_provider()?;
        Ok(match choice {
            ProviderChoice::Local => Self::Local,
            ProviderChoice::Unconfigured => Self::Disabled,
            ProviderChoice::Remote(provider) => Self::Remote {
                model: settings.model_or_default_for(&provider),
                provider,
            },
        })
    }

    /// Kaynağın `dim` boyutlu vektörler için kimliği; embedding kapalıysa yok.
    pub fn identity(&self, dim: usize) -> Option<EmbeddingIdentity> {
        match self {
            Self::Disabled => None,
            Self::Fixture(identity) => Some(identity.clone()),
            Self::Local => Some(EmbeddingIdentity::local(&DEFAULT_LOCAL_MODEL)),
            Self::Remote { provider, model } => Some(EmbeddingIdentity {
                provider: provider_label(provider).to_string(),
                model: model.clone(),
                revision: None,
                dim,
            }),
        }
    }

    /// İnsan okur özet (hata ve tanılama mesajları için).
    pub fn describe(&self) -> String {
        match self {
            Self::Disabled => "disabled".to_string(),
            Self::Fixture(identity) => identity.to_string(),
            Self::Local => EmbeddingIdentity::local(&DEFAULT_LOCAL_MODEL).to_string(),
            Self::Remote { provider, model } => format!("{} {}", provider_label(provider), model),
        }
    }

    /// Kayıtlı vektörler bu kaynakla karıştırılamıyorsa uyuşmazlığı döndürür.
    ///
    /// Kimliksiz (eski) indeksler kimlik kaydı başlamadan önce yalnızca uzak
    /// sağlayıcı ya da fixture ile kurulabiliyordu: bu kaynaklarla uyumlu
    /// sayılır, yerel modelle değil. Uzak kaynağın boyutu önceden bilinmediği
    /// için sağlayıcı ve model adı karşılaştırılır.
    pub fn mismatch_with(
        &self,
        recorded: Option<&EmbeddingIdentity>,
    ) -> Option<EmbeddingIdentityMismatch> {
        let compatible = match (self, recorded) {
            (Self::Disabled, _) => true,
            (Self::Fixture(_), None) | (Self::Remote { .. }, None) => true,
            (Self::Local, None) => false,
            (Self::Fixture(identity), Some(recorded)) => identity == recorded,
            (Self::Local, Some(recorded)) => {
                *recorded == EmbeddingIdentity::local(&DEFAULT_LOCAL_MODEL)
            }
            (Self::Remote { provider, model }, Some(recorded)) => {
                recorded.provider == provider_label(provider)
                    && recorded.model == *model
                    && recorded.revision.is_none()
            }
        };
        if compatible {
            None
        } else {
            Some(EmbeddingIdentityMismatch {
                recorded: recorded.cloned(),
                configured: self.describe(),
            })
        }
    }
}

/// İndeksteki vektörler yapılandırılmış embedder'dan farklı bir kaynaktan
/// geliyor; aynı tabloda karıştırılamazlar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingIdentityMismatch {
    /// Manifestteki kimlik; eski indekslerde yok.
    pub recorded: Option<EmbeddingIdentity>,
    /// Yapılandırılmış kaynağın özeti.
    pub configured: String,
}

impl std::fmt::Display for EmbeddingIdentityMismatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.recorded {
            Some(recorded) => write!(
                formatter,
                "the vector index was built with {} but the configured embedder is {}; re-index the project (`ccm-cli index` or the index_project tool) to re-embed it once",
                recorded, self.configured
            ),
            None => write!(
                formatter,
                "the vector index has no vectors from the configured embedder {} (it was built by an older CCM or without embeddings); re-index the project (`ccm-cli index` or the index_project tool) to embed it once",
                self.configured
            ),
        }
    }
}

impl std::error::Error for EmbeddingIdentityMismatch {}

/// Yapılandırılmış embedder: uzak HTTP servisi ya da süreç içi yerel model.
pub enum Embedder {
    Remote(RemoteEmbedder),
    #[cfg(not(all(target_os = "macos", target_arch = "x86_64")))]
    Local(std::sync::Arc<crate::vector::local::LocalEmbedder>),
}

impl Embedder {
    /// Ortamdaki yapılandırmaya göre embedder'ı kurar. Yerel model süreç
    /// başına bir kez yüklenir; dosyaları eksikse ilk kullanımda indirilir.
    pub async fn from_env() -> Result<Self> {
        let (choice, settings) = configured_provider()?;
        match choice {
            ProviderChoice::Remote(provider) => RemoteEmbedder::configured(
                provider.clone(),
                settings.host_or_default_for(&provider),
                settings.model_or_default_for(&provider),
            )
            .map(Self::Remote)
            .context(
                "Embedder not initialized. Configure EMBEDDING_PROVIDER/EMBEDDING_HOST/EMBEDDING_MODEL and EMBEDDING_API_KEY (or OPENAI_API_KEY), or disable semantic search with CCM_DISABLE_EMBEDDER=1.",
            ),
            ProviderChoice::Local => local_embedder().await,
            ProviderChoice::Unconfigured => Err(anyhow::anyhow!(EMBEDDER_UNCONFIGURED_REASON)),
        }
    }

    /// Vektör deposunun tek `embed` çağrısına verdiği en fazla metin sayısı:
    /// uzak sağlayıcıda bir HTTP isteği; yerel model çağrıyı kendi çıkarım
    /// batch'lerine böler.
    pub fn texts_per_call(&self) -> Result<usize> {
        match self {
            Self::Remote(_) => remote_batch_size_from_env(),
            #[cfg(not(all(target_os = "macos", target_arch = "x86_64")))]
            Self::Local(local) => Ok(local.texts_per_call()),
        }
    }

    /// Metinleri giriş sırasıyla embed eder.
    pub async fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        match self {
            Self::Remote(remote) => remote.embed(texts).await,
            #[cfg(not(all(target_os = "macos", target_arch = "x86_64")))]
            Self::Local(local) => std::sync::Arc::clone(local).embed(texts).await,
        }
    }

    /// Tanılama çıktısı için kısa özet.
    pub fn describe(&self) -> String {
        match self {
            Self::Remote(remote) => remote.endpoint_summary(),
            #[cfg(not(all(target_os = "macos", target_arch = "x86_64")))]
            Self::Local(local) => local.describe(),
        }
    }
}

#[cfg(not(all(target_os = "macos", target_arch = "x86_64")))]
async fn local_embedder() -> Result<Embedder> {
    crate::vector::local::shared_local_embedder()
        .await
        .map(Embedder::Local)
}

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
async fn local_embedder() -> Result<Embedder> {
    Err(LocalEmbedderUnsupported.into())
}

#[cfg(test)]
mod tests {
    use super::{
        choose_provider, EmbeddingIdentity, EmbeddingSource, LocalEmbedderUnsupported,
        ProviderChoice, ProviderSettings,
    };
    use crate::vector::local_model::DEFAULT_LOCAL_MODEL;
    use crate::vector::remote::Provider;

    fn settings(
        provider: Option<&str>,
        host: Option<&str>,
        model: Option<&str>,
    ) -> ProviderSettings {
        ProviderSettings {
            provider: provider.map(str::to_string),
            host: host.map(str::to_string),
            model: model.map(str::to_string),
            openai_key_in_user_env_file: false,
        }
    }

    #[test]
    fn unconfigured_environment_selects_the_local_model() {
        assert_eq!(
            choose_provider(&settings(None, None, None), true).unwrap(),
            ProviderChoice::Local
        );
    }

    #[test]
    fn unconfigured_environment_without_local_support_is_unconfigured() {
        assert_eq!(
            choose_provider(&settings(None, None, None), false).unwrap(),
            ProviderChoice::Unconfigured
        );
    }

    #[test]
    fn user_env_file_openai_key_prefers_openai_over_the_local_default() {
        let mut configured = settings(None, None, None);
        configured.openai_key_in_user_env_file = true;
        assert_eq!(
            choose_provider(&configured, true).unwrap(),
            ProviderChoice::Remote(Provider::OpenAI)
        );
        assert_eq!(
            choose_provider(&configured, false).unwrap(),
            ProviderChoice::Remote(Provider::OpenAI)
        );
        assert_eq!(
            configured.host_or_default_for(&Provider::OpenAI),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            configured.model_or_default_for(&Provider::OpenAI),
            "text-embedding-3-small"
        );

        // Yalnızca model verilmişse OpenAI'ın modeli değişir; açık host eski
        // çözümü korur.
        let mut model_only = settings(None, None, Some("text-embedding-3-large"));
        model_only.openai_key_in_user_env_file = true;
        assert_eq!(
            choose_provider(&model_only, true).unwrap(),
            ProviderChoice::Remote(Provider::OpenAI)
        );
        assert_eq!(
            model_only.host_or_default_for(&Provider::OpenAI),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            model_only.model_or_default_for(&Provider::OpenAI),
            "text-embedding-3-large"
        );
        let mut with_host = settings(None, Some("http://127.0.0.1:11434"), None);
        with_host.openai_key_in_user_env_file = true;
        assert_eq!(
            choose_provider(&with_host, true).unwrap(),
            ProviderChoice::Remote(Provider::Ollama)
        );
    }

    #[test]
    fn host_or_model_without_provider_keeps_the_previous_resolution() {
        assert_eq!(
            choose_provider(&settings(None, Some("http://127.0.0.1:11434"), None), true).unwrap(),
            ProviderChoice::Remote(Provider::Ollama)
        );
        assert_eq!(
            choose_provider(&settings(None, None, Some("nomic-embed-text")), true).unwrap(),
            ProviderChoice::Remote(Provider::Ollama)
        );
        assert_eq!(
            choose_provider(
                &settings(None, Some("https://api.openai.com/v1"), None),
                true
            )
            .unwrap(),
            ProviderChoice::Remote(Provider::OpenAI)
        );
    }

    #[test]
    fn openai_compatible_custom_provider_keeps_legacy_defaults() {
        let configured = settings(Some("voyage"), None, None);
        assert_eq!(
            choose_provider(&configured, true).unwrap(),
            ProviderChoice::Remote(Provider::OpenAI)
        );
        assert_eq!(
            configured.host_or_default_for(&Provider::OpenAI),
            "http://127.0.0.1:11434"
        );
        assert_eq!(
            configured.model_or_default_for(&Provider::OpenAI),
            "mxbai-embed-large"
        );

        let custom_host = settings(None, Some("https://embeddings.example/v1/embeddings"), None);
        assert_eq!(
            choose_provider(&custom_host, true).unwrap(),
            ProviderChoice::Remote(Provider::OpenAI)
        );
        assert_eq!(
            custom_host.model_or_default_for(&Provider::OpenAI),
            "mxbai-embed-large"
        );
    }

    #[test]
    fn explicit_provider_wins() {
        assert_eq!(
            choose_provider(&settings(Some("LOCAL"), None, None), true).unwrap(),
            ProviderChoice::Local
        );
        assert_eq!(
            choose_provider(&settings(Some("ollama"), None, None), true).unwrap(),
            ProviderChoice::Remote(Provider::Ollama)
        );
        assert_eq!(
            choose_provider(
                &settings(Some("openai"), None, Some("text-embedding-3-small")),
                true
            )
            .unwrap(),
            ProviderChoice::Remote(Provider::OpenAI)
        );
        assert_eq!(
            choose_provider(
                &settings(Some("local"), None, Some(DEFAULT_LOCAL_MODEL.repo)),
                true
            )
            .unwrap(),
            ProviderChoice::Local
        );
    }

    #[test]
    fn local_provider_rejects_unsupported_platforms_and_other_models() {
        let unsupported = choose_provider(&settings(Some("local"), None, None), false)
            .expect_err("local must not be accepted where it is compiled out");
        assert!(unsupported.is::<LocalEmbedderUnsupported>());
        assert!(
            choose_provider(
                &settings(Some("local"), None, Some("mxbai-embed-large")),
                true
            )
            .is_err(),
            "the local model is pinned; another EMBEDDING_MODEL is a conflict"
        );
    }

    fn remote(model: &str) -> EmbeddingSource {
        EmbeddingSource::Remote {
            provider: Provider::Ollama,
            model: model.to_string(),
        }
    }

    fn recorded_remote(model: &str, dim: usize) -> EmbeddingIdentity {
        EmbeddingIdentity {
            provider: "ollama".to_string(),
            model: model.to_string(),
            revision: None,
            dim,
        }
    }

    #[test]
    fn same_identity_is_compatible_and_a_changed_model_is_not() {
        let local = EmbeddingIdentity::local(&DEFAULT_LOCAL_MODEL);
        assert_eq!(EmbeddingSource::Local.mismatch_with(Some(&local)), None);
        assert_eq!(
            remote("mxbai-embed-large")
                .mismatch_with(Some(&recorded_remote("mxbai-embed-large", 1024))),
            None
        );

        let mismatch = EmbeddingSource::Local
            .mismatch_with(Some(&recorded_remote("mxbai-embed-large", 1024)))
            .expect("remote vectors must not be mixed with local ones");
        assert!(mismatch.to_string().contains("mxbai-embed-large"));
        assert!(remote("nomic-embed-text")
            .mismatch_with(Some(&recorded_remote("mxbai-embed-large", 1024)))
            .is_some());
        assert!(remote("mxbai-embed-large")
            .mismatch_with(Some(&local))
            .is_some());
        let mut other_revision = local.clone();
        other_revision.revision = Some("0".repeat(40));
        assert!(EmbeddingSource::Local
            .mismatch_with(Some(&other_revision))
            .is_some());
    }

    #[test]
    fn legacy_indexes_without_identity_only_match_pre_existing_sources() {
        assert_eq!(remote("mxbai-embed-large").mismatch_with(None), None);
        assert_eq!(EmbeddingSource::Disabled.mismatch_with(None), None);
        assert!(
            EmbeddingSource::Local.mismatch_with(None).is_some(),
            "an index without identity was never built by the local model"
        );
    }
}
