use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::json;
use std::env;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Embedding servisine ağ düzeyinde ulaşılamadı (bağlantı reddi, DNS, zaman aşımı).
/// İndeksleyici bu durumda grafı yine de aktive eder ve semantik katmanı beklemeye
/// alır; model/kimlik doğrulama gibi yapılandırma hataları bu türe girmez.
#[derive(Debug)]
pub struct EmbedderUnavailable {
    pub endpoint: String,
    pub detail: String,
}

impl std::fmt::Display for EmbedderUnavailable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "embedding service at {} is unreachable ({}). Start it (for Ollama: `ollama serve`) and re-index",
            self.endpoint, self.detail
        )
    }
}

impl std::error::Error for EmbedderUnavailable {}

/// Hata zincirinde embedding kaynağının erişilemez olduğunu bildiren bir hata
/// (servis kapalı ya da yerel model indirilemedi) olup olmadığını bildirir.
pub fn is_embedder_unavailable(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause.is::<EmbedderUnavailable>()
            || cause.is::<crate::vector::local_model::ModelDownloadFailed>()
    })
}

/// `EMBEDDING_HOST` tanımsızken kullanılan yerel Ollama adresi.
pub const DEFAULT_OLLAMA_HOST: &str = "http://127.0.0.1:11434";

/// OpenAI için varsayılan API adresi.
pub const DEFAULT_OPENAI_HOST: &str = "https://api.openai.com/v1";

/// OpenAI için varsayılan model. External benchmark'ta (`benchmarks/`)
/// `text-embedding-3-large`'ı geçti; ayrıca ~6,5 kat ucuz ve vektörleri yarı
/// boyuttadır.
pub const DEFAULT_OPENAI_MODEL: &str = "text-embedding-3-small";

/// Ollama için `EMBEDDING_MODEL` tanımsızken kullanılan uyumluluk modeli.
pub const DEFAULT_REMOTE_MODEL: &str = "mxbai-embed-large";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provider {
    OpenAI,
    Ollama,
}

pub struct RemoteEmbedder {
    client: Client,
    api_key: String,
    model: String,
    base_url: String,
    provider: Provider,
    timeout: Duration,
    max_embed_chars: usize,
}

impl RemoteEmbedder {
    pub fn new(
        api_key: String,
        model: String,
        base_url: String,
        provider: Provider,
    ) -> Result<Self> {
        let timeout_secs = env::var("EMBEDDING_TIMEOUT_SECS")
            .or_else(|_| env::var("CCM_EMBEDDING_TIMEOUT_SECS"))
            .ok()
            .and_then(|val| val.parse::<u64>().ok())
            .filter(|val| *val > 0)
            .unwrap_or(30);
        let timeout = Duration::from_secs(timeout_secs);
        let client =
            build_http_client(&|builder: reqwest::ClientBuilder| builder.timeout(timeout))?;
        let max_embed_chars: usize = env::var("CCM_MAX_EMBED_CHARS")
            .ok()
            .and_then(|val| val.parse::<usize>().ok())
            .filter(|val| *val > 0)
            .unwrap_or(6000);

        Ok(Self {
            client,
            api_key,
            model,
            base_url,
            provider,
            timeout,
            max_embed_chars,
        })
    }

    /// Seçilmiş sağlayıcı için embedder kurar: hedef adres doğrulanır ve API
    /// anahtarı ortamdan çözülür. Sağlayıcı seçimi `vector::embedder`'dadır.
    pub fn configured(provider: Provider, base_url: String, model: String) -> Result<Self> {
        validate_embedding_host(&base_url, &provider)?;
        let api_key = resolve_api_key(&provider)?;
        tracing::info!(
            provider = provider_label(&provider),
            host = %base_url,
            model = %model,
            "Embedding provider configured"
        );

        Self::new(api_key, model, base_url, provider)
    }

    /// Tanılama çıktısı için sağlayıcı, model ve uç nokta özeti.
    pub fn endpoint_summary(&self) -> String {
        format!(
            "{} model '{}' at {}",
            provider_label(&self.provider),
            self.model,
            self.base_url
        )
    }

    pub async fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        match self.provider {
            Provider::OpenAI => self.embed_openai(texts).await,
            Provider::Ollama => self.embed_ollama(texts).await,
        }
    }

    async fn send_with_timeout(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response> {
        match tokio::time::timeout(self.timeout, request.send()).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(error)) if error.is_connect() || error.is_timeout() => {
                // Kaynak zinciri (ör. "Connection refused") detayda korunur.
                let detail = format!("{:#}", anyhow::Error::new(error));
                Err(anyhow::Error::new(EmbedderUnavailable {
                    endpoint: self.base_url.clone(),
                    detail,
                }))
            }
            Ok(Err(error)) => Err(error).context("Failed to send embedding request"),
            Err(_) => Err(anyhow::Error::new(EmbedderUnavailable {
                endpoint: self.base_url.clone(),
                detail: format!("request timed out after {}s", self.timeout.as_secs()),
            })),
        }
    }

    async fn read_text_with_timeout(&self, response: reqwest::Response) -> Result<String> {
        match tokio::time::timeout(self.timeout, response.text()).await {
            Ok(res) => res.context("Failed to read embedding response body"),
            Err(_) => Err(anyhow::anyhow!(
                "Embedding response timed out after {}s",
                self.timeout.as_secs()
            )),
        }
    }

    async fn read_json_with_timeout(
        &self,
        response: reqwest::Response,
    ) -> Result<serde_json::Value> {
        match tokio::time::timeout(self.timeout, response.json()).await {
            Ok(res) => res.context("Failed to parse embedding response JSON"),
            Err(_) => Err(anyhow::anyhow!(
                "Embedding response timed out after {}s",
                self.timeout.as_secs()
            )),
        }
    }

    async fn embed_openai(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        let url = if self.base_url.ends_with("/embeddings") {
            self.base_url.clone()
        } else {
            format!("{}/embeddings", self.base_url.trim_end_matches('/'))
        };

        // External API kuralı: en az 1 retry + uyarı, son hata açıkça yükseltilir.
        let mut last_error: Option<anyhow::Error> = None;
        for attempt in 0..2 {
            match self
                .send_with_timeout(
                    self.client
                        .post(&url)
                        .header("Authorization", format!("Bearer {}", self.api_key))
                        .header("Content-Type", "application/json")
                        .json(&json!({
                            "input": texts,
                            "model": self.model
                        })),
                )
                .await
            {
                Ok(response) if response.status().is_success() => {
                    let body = self.read_json_with_timeout(response).await?;
                    let mut embeddings = Vec::new();
                    if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
                        for item in data {
                            if let Some(embedding_val) =
                                item.get("embedding").and_then(|e| e.as_array())
                            {
                                let vec: Vec<f32> = embedding_val
                                    .iter()
                                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                                    .collect();
                                embeddings.push(vec);
                            }
                        }
                    } else {
                        return Err(anyhow::anyhow!("Invalid response format from API"));
                    }
                    return Ok(embeddings);
                }
                Ok(response) => {
                    let error_text = self.read_text_with_timeout(response).await?;
                    last_error = Some(anyhow::anyhow!("Remote API Error: {}", error_text));
                }
                Err(e) => {
                    last_error = Some(e);
                }
            }
            if attempt == 0 {
                tracing::warn!(attempt, error = %last_error.as_ref().unwrap(), "OpenAI embed retry");
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("OpenAI embedding request failed")))
    }

    async fn embed_ollama(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        // Ollama native API: POST /api/embed (Newer endpoint with batch support)
        // Body: { "model": "...", "input": ["..."] }

        if self.base_url.contains("/v1") {
            // Use OpenAI format if user explicitly points to /v1
            return self.embed_openai(texts).await;
        }

        let url = if self.base_url.ends_with("/api/embed") {
            self.base_url.clone()
        } else if self.base_url.ends_with("/api/embeddings") {
            self.base_url.replace("/api/embeddings", "/api/embed")
        } else {
            format!("{}/api/embed", self.base_url.trim_end_matches('/'))
        };

        // Retry logic loop (max 1 retry for auto-pull)
        for attempt in 0..2 {
            let response_res = self
                .send_with_timeout(
                    self.client
                        .post(&url)
                        .header("Authorization", format!("Bearer {}", self.api_key))
                        .header("Content-Type", "application/json")
                        .json(&json!({
                            "model": self.model,
                            "input": texts.iter().map(|t| {
                                if t.len() > self.max_embed_chars {
                                    // Safe char boundary truncation
                                    t.chars().take(self.max_embed_chars).collect::<String>()
                                } else {
                                    t.clone()
                                }
                            }).collect::<Vec<String>>()
                        })),
                )
                .await;

            // Ağ hataları `EmbedderUnavailable` olarak yukarı taşınır (çağıran
            // graf-öncelikli indekse geçebilsin); mesaj `ollama serve` önerir.
            let response = response_res?;

            if !response.status().is_success() {
                let error_text = self.read_text_with_timeout(response).await?;

                // Auto-recovery: If it's the first attempt and model not found, try to pull it
                if attempt == 0 && error_text.contains("model") && error_text.contains("not found")
                {
                    tracing::warn!(
                        model = %self.model,
                        "Model not found in Ollama. Attempting to pull automatically."
                    );
                    tracing::info!(
                        "This may take a few minutes depending on model size and your internet speed."
                    );

                    let pull_url = format!("{}/api/pull", self.base_url.trim_end_matches('/'));
                    let pull_res = self
                        .send_with_timeout(
                            self.client
                                .post(&pull_url)
                                .json(&json!({ "name": self.model })),
                        )
                        .await;

                    match pull_res {
                        Ok(res) => {
                            if res.status().is_success() {
                                // CRITICAL: Ollama returns a STREAMING response for pull.
                                // We MUST consume the entire body to wait for download to complete.
                                let body =
                                    self.read_text_with_timeout(res).await.unwrap_or_default();

                                // Başarı kriteri yalnızca "success" durumudur; "pulling"
                                // satırları ilerlemeyi gösterir ama tamamlanmayı kanıtlamaz.
                                if body.contains("\"status\":\"success\"") {
                                    tracing::info!(
                                        model = %self.model,
                                        "Model pulled successfully. Retrying embedding."
                                    );
                                    // Continue to next loop iteration (retry)
                                    continue;
                                } else {
                                    tracing::warn!(
                                        response = %body.lines().last().unwrap_or("empty"),
                                        "Pull completed but may have failed"
                                    );
                                }
                            } else {
                                tracing::warn!(
                                    status = %res.status(),
                                    "Failed to auto-pull model"
                                );
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "Failed to connect to Ollama for pull")
                        }
                    }
                }

                return Err(anyhow::anyhow!("Ollama API Error: {}", error_text));
            }

            let body = self.read_json_with_timeout(response).await?;

            // Response format: { "embeddings": [[...], [...]] }
            if let Some(embeddings_arr) = body.get("embeddings").and_then(|e| e.as_array()) {
                let mut result_embeddings = Vec::new();
                for item in embeddings_arr {
                    if let Some(vec_vals) = item.as_array() {
                        let vec: Vec<f32> = vec_vals
                            .iter()
                            .filter_map(|v| v.as_f64().map(|f| f as f32))
                            .collect();
                        result_embeddings.push(vec);
                    }
                }
                return Ok(result_embeddings);
            } else {
                // Fallback for older API or single errors?
                // Try "embedding" field just in case single input was treated differently
                if let Some(embedding_val) = body.get("embedding").and_then(|e| e.as_array()) {
                    let vec: Vec<f32> = embedding_val
                        .iter()
                        .filter_map(|v| v.as_f64().map(|f| f as f32))
                        .collect();
                    return Ok(vec![vec]);
                } else {
                    return Err(anyhow::anyhow!(
                        "Invalid response format from Ollama API (expected 'embeddings')"
                    ));
                }
            }
        }

        Err(anyhow::anyhow!("Failed after retries"))
    }
}

/// Operatörün `~/.ccm/.env` dosyası; HOME/USERPROFILE tanımsızsa ya da dosya
/// yoksa `None`.
fn user_env_file() -> Option<PathBuf> {
    let home = env::var("HOME").or_else(|_| env::var("USERPROFILE")).ok()?;
    let global_config = PathBuf::from(home).join(".ccm").join(".env");
    global_config.exists().then_some(global_config)
}

/// Operatörün `~/.ccm/.env` dosyasını süreç ortamına yükler; zaten tanımlı
/// değişkenler (host config'inin `env`'i) ezilmez. Güvenlik: repo cwd'sindeki
/// `.env` asla yüklenmez; güvenilmeyen bir repo `EMBEDDING_HOST`'u saldırgana
/// çevirip kaynak kodu dışarı gönderebilir.
pub fn load_user_env_file() -> Result<()> {
    let Some(global_config) = user_env_file() else {
        return Ok(());
    };
    dotenvy::from_path(&global_config)
        .with_context(|| format!("Failed to load {}", global_config.display()))
}

/// `~/.ccm/.env` boş olmayan bir `OPENAI_API_KEY` tanımlıyor mu? OpenAI'ın
/// örtük seçimi yalnızca bu dosyadaki anahtara dayanır: kabukta global olarak
/// export edilmiş bir anahtar, kullanıcı CCM için onay vermeden kodu dışarı
/// göndermemelidir.
pub fn user_env_file_defines_openai_key() -> Result<bool> {
    match user_env_file() {
        Some(global_config) => env_file_defines_openai_key(&global_config),
        None => Ok(false),
    }
}

/// Dosyadaki ilk `OPENAI_API_KEY` tanımı boş değilse `true`; dotenvy de ilk
/// tanımı yükler.
fn env_file_defines_openai_key(path: &Path) -> Result<bool> {
    let entries = dotenvy::from_path_iter(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    for entry in entries {
        let (key, value) = entry.with_context(|| format!("Failed to parse {}", path.display()))?;
        if key == "OPENAI_API_KEY" {
            return Ok(!value.trim().is_empty());
        }
    }
    Ok(false)
}

/// Embedding isteği gönderilecek hedefi doğrular. Loopback hedefleri ve açıkça
/// seçilmiş OpenAI sağlayıcısının resmi HTTPS API adresi doğrudan kabul edilir;
/// diğer dış hedefler `CCM_ALLOW_REMOTE_EMBEDDING=1` onayı ister.
fn validate_embedding_host(base_url: &str, provider: &Provider) -> Result<()> {
    let url = reqwest::Url::parse(base_url)
        .with_context(|| format!("EMBEDDING_HOST geçerli bir URL değil: {}", base_url))?;
    let scheme_ok = matches!(url.scheme(), "http" | "https");
    if !scheme_ok {
        return Err(anyhow::anyhow!(
            "EMBEDDING_HOST yalnızca http/https kabul eder: {}",
            base_url
        ));
    }
    let host = url
        .host_str()
        .with_context(|| format!("EMBEDDING_HOST host içermiyor: {}", base_url))?;
    // Yalnızca tam bir IP adresi loopback olabilir: "127.0.0.1.evil.com" bir
    // alan adıdır ve metin önekiyle loopback sayılmamalıdır.
    let is_loopback = match host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<std::net::IpAddr>()
    {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_loopback() || ip.is_unspecified(),
        Ok(std::net::IpAddr::V6(ip)) => ip.is_loopback(),
        Err(_) => false,
    };

    // `localhost.` gibi son noktalı yerel adlar da loopback kabul edilir.
    let normalized = host.trim_end_matches('.').to_ascii_lowercase();
    let local_name = normalized == "localhost" || normalized == "localhost.localdomain";
    if is_loopback || local_name {
        return Ok(());
    }

    let canonical_openai = *provider == Provider::OpenAI
        && url.scheme() == "https"
        && normalized == "api.openai.com"
        && url.port_or_known_default() == Some(443)
        && matches!(url.path(), "/v1" | "/v1/" | "/v1/embeddings")
        && url.query().is_none()
        && url.fragment().is_none();
    if canonical_openai {
        return Ok(());
    }

    let allowed = env::var("CCM_ALLOW_REMOTE_EMBEDDING")
        .map(|value| value == "1")
        .unwrap_or(false);
    if allowed {
        return Ok(());
    }
    Err(anyhow::anyhow!(
        "EMBEDDING_HOST '{}' loopback dışı bir hedefe işaret ediyor; \
         güvenilir bir dış embedding servisi kullanıyorsanız CCM_ALLOW_REMOTE_EMBEDDING=1 ayarlayın",
        base_url
    ))
}

/// Sağlayıcı adı ve host bilgisini desteklenen HTTP sözleşmesine çözer.
pub(crate) fn resolve_provider(provider_str: &str, base_url: &str) -> Provider {
    // OpenAI-uyumlu /embeddings sözleşmesi kullanan sağlayıcılar tek kod
    // yolundan geçer: OpenAI, Azure OpenAI, HuggingFace TEI, Voyage, Jina,
    // LM Studio, llama.cpp server, LocalAI vb. Açıkça ollama belirtilmedikçe
    // bu sağlayıcı adları OpenAI provider'ına çözülür.
    let normalized = provider_str.to_lowercase();
    let openai_compatible = normalized.contains("openai")
        || normalized.contains("tei")
        || normalized.contains("text-embeddings-inference")
        || normalized.contains("voyage")
        || normalized.contains("jina")
        || normalized.contains("lmstudio")
        || normalized.contains("localai")
        || normalized.contains("llama.cpp")
        || base_url.contains("api.openai.com")
        || base_url.contains("inference.ai.azure.com")
        || base_url.contains("/v1/embeddings");
    if openai_compatible {
        return Provider::OpenAI;
    }
    Provider::Ollama
}

/// Ortam değişkenini yalnızca boş olmayan bir değer içeriyorsa döndürür.
fn nonempty_env(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Seçilen uzak sağlayıcının API anahtarını ortamdan çözer.
fn resolve_api_key(provider: &Provider) -> Result<String> {
    match provider {
        Provider::OpenAI => nonempty_env("EMBEDDING_API_KEY")
            .or_else(|| nonempty_env("OPENAI_API_KEY"))
            .context("EMBEDDING_API_KEY or OPENAI_API_KEY not set"),
        Provider::Ollama => Ok(nonempty_env("EMBEDDING_API_KEY")
            .or_else(|| nonempty_env("OPENAI_API_KEY"))
            .unwrap_or_else(|| "ollama".to_string())),
    }
}

/// Manifest ve tanılama çıktılarında kullanılan kararlı sağlayıcı etiketi.
pub fn provider_label(provider: &Provider) -> &'static str {
    match provider {
        Provider::OpenAI => "openai",
        Provider::Ollama => "ollama",
    }
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "Unknown panic payload".to_string()
    }
}

/// HTTP istemcisini kurar; sistem proxy ayarları okunurken hata ya da panik
/// olursa proxy'siz yeniden dener. `configure` zaman aşımı gibi ayarları uygular.
pub(crate) fn build_http_client(
    configure: &dyn Fn(reqwest::ClientBuilder) -> reqwest::ClientBuilder,
) -> Result<Client> {
    match catch_unwind(AssertUnwindSafe(|| configure(Client::builder()).build())) {
        Ok(Ok(client)) => return Ok(client),
        Ok(Err(error)) => {
            tracing::warn!(
                error = %error,
                "Failed to build HTTP client with system proxy settings; retrying with no_proxy"
            );
        }
        Err(payload) => {
            let msg = panic_message(&payload);
            tracing::error!(
                panic = %msg,
                "HTTP client builder panicked with system proxy settings; retrying with no_proxy"
            );
        }
    }

    match catch_unwind(AssertUnwindSafe(|| {
        configure(Client::builder()).no_proxy().build()
    })) {
        Ok(Ok(client)) => Ok(client),
        Ok(Err(error)) => Err(anyhow::anyhow!(
            "Failed to build HTTP client in no_proxy mode: {}",
            error
        )),
        Err(payload) => {
            let msg = panic_message(&payload);
            tracing::error!(panic = %msg, "HTTP client builder panicked in no_proxy mode");
            Err(anyhow::anyhow!(
                "HTTP client builder panicked in no_proxy mode: {}",
                msg
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_http_client, env_file_defines_openai_key, panic_message, provider_label,
        resolve_api_key, resolve_provider, validate_embedding_host, Provider,
    };
    use std::sync::Mutex;
    use std::time::Duration;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn resolve_provider_defaults_to_ollama_for_localhost() {
        let provider = resolve_provider("", "http://127.0.0.1:11434");
        assert_eq!(provider, Provider::Ollama);
    }

    #[test]
    fn resolve_provider_uses_openai_when_explicit() {
        let provider = resolve_provider("openai", "http://127.0.0.1:11434");
        assert_eq!(provider, Provider::OpenAI);
    }

    #[test]
    fn resolve_provider_uses_openai_when_host_is_openai() {
        let provider = resolve_provider("", "https://api.openai.com/v1");
        assert_eq!(provider, Provider::OpenAI);
    }

    #[test]
    fn embedding_host_rejects_non_loopback_without_explicit_consent() {
        // Ortam değişkenleri süreç genelinde paylaşılır; diğer testlerle yarışmamak
        // için ENV_LOCK altında değiştirilir.
        let _guard = ENV_LOCK.lock().unwrap();
        assert!(validate_embedding_host("http://127.0.0.1:11434", &Provider::Ollama).is_ok());
        assert!(validate_embedding_host("http://localhost:8080", &Provider::Ollama).is_ok());
        assert!(
            validate_embedding_host("http://localhost.localdomain:8080", &Provider::Ollama).is_ok()
        );
        assert!(validate_embedding_host("http://0.0.0.0:8080", &Provider::Ollama).is_ok());
        assert!(validate_embedding_host("http://[::1]:11434", &Provider::Ollama).is_ok());
        assert!(validate_embedding_host("ftp://127.0.0.1", &Provider::Ollama).is_err());
        assert!(validate_embedding_host("not-a-url", &Provider::Ollama).is_err());
        // Resmi OpenAI adresi yalnızca OpenAI sağlayıcısıyla güvenilir varsayılandır.
        assert!(validate_embedding_host("https://api.openai.com/v1", &Provider::OpenAI).is_ok());
        assert!(validate_embedding_host("https://api.openai.com/v1", &Provider::Ollama).is_err());
        assert!(validate_embedding_host("https://10.0.0.1", &Provider::OpenAI).is_err());
        // IP gibi başlayan alan adları loopback değildir.
        assert!(
            validate_embedding_host("http://127.0.0.1.evil.com:11434", &Provider::Ollama).is_err()
        );
        assert!(validate_embedding_host("http://localhost.evil.com", &Provider::Ollama).is_err());
        // Açık onay ile dış hedef kabul edilir.
        std::env::set_var("CCM_ALLOW_REMOTE_EMBEDDING", "1");
        assert!(validate_embedding_host("https://example.com/v1", &Provider::OpenAI).is_ok());
        std::env::remove_var("CCM_ALLOW_REMOTE_EMBEDDING");
    }

    #[test]
    fn provider_label_matches_enum() {
        assert_eq!(provider_label(&Provider::OpenAI), "openai");
        assert_eq!(provider_label(&Provider::Ollama), "ollama");
    }

    #[test]
    fn api_key_resolution_uses_env_or_ollama_default() {
        let _guard = ENV_LOCK.lock().unwrap();
        let previous_embedding = std::env::var("EMBEDDING_API_KEY").ok();
        let previous_openai = std::env::var("OPENAI_API_KEY").ok();

        std::env::remove_var("EMBEDDING_API_KEY");
        std::env::remove_var("OPENAI_API_KEY");
        assert_eq!(resolve_api_key(&Provider::Ollama).unwrap(), "ollama");
        assert!(resolve_api_key(&Provider::OpenAI).is_err());

        std::env::set_var("OPENAI_API_KEY", "   ");
        assert!(resolve_api_key(&Provider::OpenAI).is_err());
        std::env::remove_var("OPENAI_API_KEY");

        std::env::set_var("EMBEDDING_API_KEY", "test-key");
        assert_eq!(resolve_api_key(&Provider::OpenAI).unwrap(), "test-key");
        assert_eq!(resolve_api_key(&Provider::Ollama).unwrap(), "test-key");

        match previous_embedding {
            Some(value) => std::env::set_var("EMBEDDING_API_KEY", value),
            None => std::env::remove_var("EMBEDDING_API_KEY"),
        }
        match previous_openai {
            Some(value) => std::env::set_var("OPENAI_API_KEY", value),
            None => std::env::remove_var("OPENAI_API_KEY"),
        }
    }

    #[test]
    fn panic_message_decodes_common_payloads() {
        let str_payload: Box<dyn std::any::Any + Send> = Box::new("boom");
        assert_eq!(panic_message(&str_payload), "boom");

        let string_payload: Box<dyn std::any::Any + Send> = Box::new("custom".to_string());
        assert_eq!(panic_message(&string_payload), "custom");

        let other_payload: Box<dyn std::any::Any + Send> = Box::new(42u32);
        assert_eq!(panic_message(&other_payload), "Unknown panic payload");
    }

    #[test]
    fn http_client_builder_succeeds_with_timeout() {
        assert!(build_http_client(
            &|builder: reqwest::ClientBuilder| builder.timeout(Duration::from_secs(5))
        )
        .is_ok());
    }

    #[test]
    fn env_file_openai_key_follows_the_first_definition() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join(".env");
        let defines = |content: &str| -> anyhow::Result<bool> {
            std::fs::write(&path, content)?;
            env_file_defines_openai_key(&path)
        };
        assert!(defines(
            "EMBEDDING_PROVIDER=openai\nOPENAI_API_KEY=sk-test\n"
        )?);
        assert!(!defines("EMBEDDING_API_KEY=sk-test\n")?);
        assert!(!defines("OPENAI_API_KEY=   \n")?);
        assert!(!defines("OPENAI_API_KEY=\nOPENAI_API_KEY=sk-test\n")?);
        Ok(())
    }
}
