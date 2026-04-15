use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::json;
use std::env;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Duration;

#[derive(Debug, Clone)]
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
        let client = build_http_client(timeout)?;

        Ok(Self {
            client,
            api_key,
            model,
            base_url,
            provider,
            timeout,
        })
    }

    pub fn from_env() -> Result<Self> {
        use std::path::PathBuf;
        let _ = dotenvy::dotenv();

        // Load global config from ~/.ccm/.env if it exists
        if let Ok(home) = env::var("HOME") {
            let global_config = PathBuf::from(&home).join(".ccm").join(".env");
            if global_config.exists() {
                // eprintln!("Loading global config from: {:?}", global_config);
                let _ = dotenvy::from_path(&global_config);
            }
        } else if let Ok(user_profile) = env::var("USERPROFILE") {
            let global_config = PathBuf::from(&user_profile).join(".ccm").join(".env");
            if global_config.exists() {
                let _ = dotenvy::from_path(&global_config);
            }
        }

        let base_url =
            env::var("EMBEDDING_HOST").unwrap_or_else(|_| "http://127.0.0.1:11434".to_string());

        // Default model: nomic-embed-text is robust and standard for local RAG
        let model = env::var("EMBEDDING_MODEL").unwrap_or_else(|_| "mxbai-embed-large".to_string());

        // Detect provider: If explicit, use it. If base_url looks like Ollama, use it.
        // OTHERWISE DEFAULT TO OLLAMA (Local First approach).
        let provider_str = env::var("EMBEDDING_PROVIDER")
            .unwrap_or_default()
            .to_lowercase();

        let provider = if provider_str.contains("openai") || base_url.contains("api.openai.com") {
            Provider::OpenAI
        } else {
            // Default to Ollama for privacy/local-first
            Provider::Ollama
        };

        let api_key = match provider {
            Provider::OpenAI => env::var("EMBEDDING_API_KEY")
                .or_else(|_| env::var("OPENAI_API_KEY"))
                .context("EMBEDDING_API_KEY or OPENAI_API_KEY not set")?,
            Provider::Ollama => env::var("EMBEDDING_API_KEY")
                .or_else(|_| env::var("OPENAI_API_KEY"))
                .unwrap_or_else(|_| "ollama".to_string()), // Default dummy key
        };

        Self::new(api_key, model, base_url, provider)
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
            Ok(res) => res.context("Failed to send embedding request"),
            Err(_) => Err(anyhow::anyhow!(
                "Embedding request timed out after {}s",
                self.timeout.as_secs()
            )),
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

        let response = self
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
            .context("Failed to send embedding request (OpenAI format)")?;

        if !response.status().is_success() {
            let error_text = self.read_text_with_timeout(response).await?;
            return Err(anyhow::anyhow!("Remote API Error: {}", error_text));
        }

        let body = self.read_json_with_timeout(response).await?;

        let mut embeddings = Vec::new();
        if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
            for item in data {
                if let Some(embedding_val) = item.get("embedding").and_then(|e| e.as_array()) {
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

        Ok(embeddings)
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
                                if t.len() > 6000 {
                                    // Safe char boundary truncation
                                    t.chars().take(6000).collect::<String>()
                                } else {
                                    t.clone()
                                }
                            }).collect::<Vec<String>>()
                        })),
                )
                .await;

            // Handle network errors first to give friendly advice
            let response = match response_res {
                Ok(resp) => resp,
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("connect") || msg.contains("refused") {
                        return Err(anyhow::anyhow!(
                            "\n❌ Could not connect to Ollama at {}.\n\
                            👉 Is Ollama running? Run `ollama serve` in a terminal.\n\
                            👉 Not installed? Download it from https://ollama.com\n",
                            self.base_url
                        ));
                    }
                    return Err(anyhow::anyhow!("Failed to connect to Ollama: {}", e));
                }
            };

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

                                // Check if last line contains "success" or download completed
                                if body.contains("\"status\":\"success\"")
                                    || body.contains("pulling")
                                {
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

fn build_http_client(timeout: Duration) -> Result<Client> {
    match catch_unwind(AssertUnwindSafe(|| {
        Client::builder().timeout(timeout).build()
    })) {
        Ok(Ok(client)) => return Ok(client),
        Ok(Err(error)) => {
            tracing::warn!(
                error = %error,
                "Failed to build HTTP client with system proxy settings; retrying with no_proxy"
            );
        }
        Err(_) => {
            tracing::warn!(
                "HTTP client builder panicked with system proxy settings; retrying with no_proxy"
            );
        }
    }

    match catch_unwind(AssertUnwindSafe(|| {
        Client::builder().timeout(timeout).no_proxy().build()
    })) {
        Ok(Ok(client)) => Ok(client),
        Ok(Err(error)) => Err(anyhow::anyhow!(
            "Failed to build HTTP client in no_proxy mode: {}",
            error
        )),
        Err(_) => Err(anyhow::anyhow!(
            "HTTP client builder panicked in no_proxy mode"
        )),
    }
}
