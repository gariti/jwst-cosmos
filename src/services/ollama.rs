//! Ollama API client for vision models and model management.

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Represents an Ollama model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaModel {
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub digest: String,
    #[serde(default)]
    pub modified_at: Option<String>,
    #[serde(default)]
    pub details: Option<ModelDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDetails {
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub parameter_size: Option<String>,
    #[serde(default)]
    pub quantization_level: Option<String>,
}

impl OllamaModel {
    /// Get human-readable size.
    pub fn size_str(&self) -> String {
        let gb = self.size as f64 / 1_073_741_824.0;
        if gb >= 1.0 {
            format!("{:.1} GB", gb)
        } else {
            let mb = self.size as f64 / 1_048_576.0;
            format!("{:.0} MB", mb)
        }
    }

    /// Check if this is a vision model (multimodal).
    pub fn is_vision_model(&self) -> bool {
        let name_lower = self.name.to_lowercase();
        name_lower.contains("llava")
            || name_lower.contains("moondream")
            || name_lower.contains("bakllava")
            || name_lower.contains("vision")
    }
}

/// Response from listing models.
#[derive(Debug, Deserialize)]
struct ModelsResponse {
    models: Vec<OllamaModel>,
}

/// Request for generating a response.
#[derive(Debug, Serialize)]
struct GenerateRequest {
    model: String,
    prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    images: Option<Vec<String>>,
    stream: bool,
}

/// Response from generation.
#[derive(Debug, Deserialize)]
struct GenerateResponse {
    response: String,
    #[serde(default)]
    done: bool,
}

/// Request for pulling a model.
#[derive(Debug, Serialize)]
struct PullRequest {
    name: String,
    stream: bool,
}

/// Progress response from pulling.
#[derive(Debug, Deserialize)]
pub struct PullProgress {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub digest: Option<String>,
    #[serde(default)]
    pub total: Option<u64>,
    #[serde(default)]
    pub completed: Option<u64>,
}

/// Service for interacting with Ollama API.
pub struct OllamaService {
    client: Client,
    base_url: Arc<RwLock<Option<String>>>,
}

impl OllamaService {
    /// Create a new Ollama service.
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .expect("Failed to create HTTP client"),
            base_url: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the base URL (from SSH tunnel).
    pub async fn set_base_url(&self, url: String) {
        let mut base_url = self.base_url.write().await;
        *base_url = Some(url);
    }

    /// Get the current base URL.
    async fn get_base_url(&self) -> Result<String> {
        let base_url = self.base_url.read().await;
        base_url
            .clone()
            .context("Ollama base URL not set - tunnel not established")
    }

    /// Check if connected to Ollama.
    pub async fn is_connected(&self) -> bool {
        if let Ok(url) = self.get_base_url().await {
            self.client
                .get(format!("{}/api/tags", url))
                .send()
                .await
                .map(|r| r.status().is_success())
                .unwrap_or(false)
        } else {
            false
        }
    }

    /// List available models.
    pub async fn list_models(&self) -> Result<Vec<OllamaModel>> {
        let base_url = self.get_base_url().await?;
        let response = self
            .client
            .get(format!("{}/api/tags", base_url))
            .send()
            .await
            .context("Failed to list models")?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to list models: {}", response.status());
        }

        let models_response: ModelsResponse = response
            .json()
            .await
            .context("Failed to parse models response")?;

        Ok(models_response.models)
    }

    /// List only vision-capable models.
    pub async fn list_vision_models(&self) -> Result<Vec<OllamaModel>> {
        let models = self.list_models().await?;
        Ok(models.into_iter().filter(|m| m.is_vision_model()).collect())
    }

    /// Generate a response with an optional image.
    pub async fn generate(
        &self,
        model: &str,
        prompt: &str,
        image_base64: Option<String>,
    ) -> Result<String> {
        let base_url = self.get_base_url().await?;

        let request = GenerateRequest {
            model: model.to_string(),
            prompt: prompt.to_string(),
            images: image_base64.map(|img| vec![img]),
            stream: false,
        };

        let response = self
            .client
            .post(format!("{}/api/generate", base_url))
            .json(&request)
            .send()
            .await
            .context("Failed to generate response")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Generation failed: {}", error_text);
        }

        let gen_response: GenerateResponse = response
            .json()
            .await
            .context("Failed to parse generation response")?;

        Ok(gen_response.response)
    }

    /// Analyze an image using a vision model.
    pub async fn analyze_image(
        &self,
        model: &str,
        image_path: &std::path::Path,
        prompt: &str,
    ) -> Result<String> {
        // Read and base64 encode the image
        let image_bytes = std::fs::read(image_path)
            .context(format!("Failed to read image: {:?}", image_path))?;
        let image_base64 = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            &image_bytes,
        );

        self.generate(model, prompt, Some(image_base64)).await
    }

    /// Pull (download) a model.
    pub async fn pull_model(&self, model_name: &str) -> Result<tokio::sync::mpsc::Receiver<PullProgress>> {
        let base_url = self.get_base_url().await?;
        let (tx, rx) = tokio::sync::mpsc::channel(100);

        let client = self.client.clone();
        let url = format!("{}/api/pull", base_url);
        let model = model_name.to_string();

        tokio::spawn(async move {
            let request = PullRequest {
                name: model,
                stream: true,
            };

            let response = match client.post(&url).json(&request).send().await {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(PullProgress {
                        status: format!("Error: {}", e),
                        digest: None,
                        total: None,
                        completed: None,
                    }).await;
                    return;
                }
            };

            let mut stream = response.bytes_stream();
            use futures_util::StreamExt;

            let mut buffer = Vec::new();
            while let Some(chunk) = stream.next().await {
                if let Ok(bytes) = chunk {
                    buffer.extend_from_slice(&bytes);

                    // Try to parse complete JSON objects from buffer
                    while let Some(newline_pos) = buffer.iter().position(|&b| b == b'\n') {
                        let line = buffer.drain(..=newline_pos).collect::<Vec<_>>();
                        if let Ok(progress) = serde_json::from_slice::<PullProgress>(&line) {
                            if tx.send(progress).await.is_err() {
                                return;
                            }
                        }
                    }
                }
            }
        });

        Ok(rx)
    }

    /// Delete a model.
    pub async fn delete_model(&self, model_name: &str) -> Result<()> {
        let base_url = self.get_base_url().await?;

        let response = self
            .client
            .delete(format!("{}/api/delete", base_url))
            .json(&serde_json::json!({ "name": model_name }))
            .send()
            .await
            .context("Failed to delete model")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Failed to delete model: {}", error_text);
        }

        Ok(())
    }

    /// Get model info.
    pub async fn show_model(&self, model_name: &str) -> Result<OllamaModel> {
        let base_url = self.get_base_url().await?;

        let response = self
            .client
            .post(format!("{}/api/show", base_url))
            .json(&serde_json::json!({ "name": model_name }))
            .send()
            .await
            .context("Failed to get model info")?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to get model info: {}", response.status());
        }

        let model: OllamaModel = response
            .json()
            .await
            .context("Failed to parse model info")?;

        Ok(model)
    }
}

impl Default for OllamaService {
    fn default() -> Self {
        Self::new()
    }
}
