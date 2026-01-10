use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::json;
use std::env;

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
}

impl RemoteEmbedder {
    pub fn new(api_key: String, model: String, base_url: String, provider: Provider) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            api_key,
            model,
            base_url,
            provider,
        }
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
        let model = env::var("EMBEDDING_MODEL").unwrap_or_else(|_| "nomic-embed-text".to_string());

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

        Ok(Self::new(api_key, model, base_url, provider))
    }

    pub async fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        match self.provider {
            Provider::OpenAI => self.embed_openai(texts).await,
            Provider::Ollama => self.embed_ollama(texts).await,
        }
    }

    async fn embed_openai(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        let url = if self.base_url.ends_with("/embeddings") {
            self.base_url.clone()
        } else {
            format!("{}/embeddings", self.base_url.trim_end_matches('/'))
        };

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&json!({
                "input": texts,
                "model": self.model
            }))
            .send()
            .await
            .context("Failed to send embedding request (OpenAI format)")?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow::anyhow!("Remote API Error: {}", error_text));
        }

        let body: serde_json::Value = response.json().await?;

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
            let response = self
                .client
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
                }))
                .send()
                .await
                .context("Failed to send embedding request (Ollama /api/embed)")?;

            if !response.status().is_success() {
                let error_text = response.text().await?;

                // Auto-recovery: If it's the first attempt and model not found, try to pull it
                if attempt == 0 && error_text.contains("model") && error_text.contains("not found")
                {
                    eprintln!("⚠️  Model '{}' not found in Ollama. Attempting to pull it automatically...", self.model);

                    let pull_url = format!("{}/api/pull", self.base_url.trim_end_matches('/'));
                    let pull_res = self
                        .client
                        .post(&pull_url)
                        .json(&json!({ "name": self.model }))
                        .send()
                        .await;

                    match pull_res {
                        Ok(res) => {
                            if res.status().is_success() {
                                eprintln!(
                                    "✅ Model '{}' pulled successfully. Retrying embedding...",
                                    self.model
                                );
                                // Continue to next loop iteration (retry)
                                continue;
                            } else {
                                eprintln!("❌ Failed to auto-pull model: {}", res.status());
                            }
                        }
                        Err(e) => eprintln!("❌ Failed to connect to Ollama for pull: {}", e),
                    }
                }

                return Err(anyhow::anyhow!("Ollama API Error: {}", error_text));
            }

            let body: serde_json::Value = response.json().await?;

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
