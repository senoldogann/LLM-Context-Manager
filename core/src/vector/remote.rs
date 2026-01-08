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
            client: Client::new(),
            api_key,
            model,
            base_url,
            provider,
        }
    }

    pub fn from_env() -> Result<Self> {
        let _ = dotenvy::dotenv();

        let api_key = env::var("EMBEDDING_API_KEY")
            .or_else(|_| env::var("OPENAI_API_KEY"))
            .context("EMBEDDING_API_KEY or OPENAI_API_KEY not set")?;

        let model =
            env::var("EMBEDDING_MODEL").unwrap_or_else(|_| "text-embedding-3-small".to_string());

        let base_url =
            env::var("EMBEDDING_HOST").unwrap_or_else(|_| "https://api.openai.com/v1".to_string());

        // Detect provider from URL or explicit generic env
        let provider_str = env::var("EMBEDDING_PROVIDER")
            .unwrap_or_default()
            .to_lowercase();
        let provider = if provider_str.contains("ollama") || base_url.contains("ollama") {
            Provider::Ollama
        } else {
            Provider::OpenAI
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
        } else {
            // Check if user accidentally put /api/embeddings and fix it, or just append /api/embed
            if self.base_url.ends_with("/api/embeddings") {
                self.base_url.replace("/api/embeddings", "/api/embed")
            } else {
                format!("{}/api/embed", self.base_url.trim_end_matches('/'))
            }
        };

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&json!({
                "model": self.model,
                "input": texts
            }))
            .send()
            .await
            .context("Failed to send embedding request (Ollama /api/embed)")?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
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
            Ok(result_embeddings)
        } else {
            // Fallback for older API or single errors?
            // Try "embedding" field just in case single input was treated differently
            if let Some(embedding_val) = body.get("embedding").and_then(|e| e.as_array()) {
                let vec: Vec<f32> = embedding_val
                    .iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect();
                Ok(vec![vec])
            } else {
                Err(anyhow::anyhow!(
                    "Invalid response format from Ollama API (expected 'embeddings')"
                ))
            }
        }
    }
}
