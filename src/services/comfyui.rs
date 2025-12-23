//! ComfyUI WebSocket client for image generation.

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

/// Generation progress information.
#[derive(Debug, Clone)]
pub struct GenerationProgress {
    pub status: String,
    pub progress: f32,
    pub current_step: u32,
    pub total_steps: u32,
    pub node_id: Option<String>,
    pub logs: Vec<String>,
}

/// Result of image generation.
#[derive(Debug, Clone)]
pub struct GenerationResult {
    pub image_path: PathBuf,
    pub prompt_id: String,
}

/// WebSocket message types from ComfyUI.
#[derive(Debug, Deserialize)]
struct WsMessage {
    #[serde(rename = "type")]
    msg_type: String,
    data: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ProgressData {
    #[serde(default)]
    value: u32,
    #[serde(default)]
    max: u32,
}

#[derive(Debug, Deserialize)]
struct ExecutingData {
    node: Option<String>,
    prompt_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExecutedData {
    node: String,
    output: Option<OutputData>,
    prompt_id: String,
}

#[derive(Debug, Deserialize)]
struct OutputData {
    images: Option<Vec<ImageOutput>>,
}

#[derive(Debug, Deserialize)]
struct ImageOutput {
    filename: String,
    subfolder: String,
    #[serde(rename = "type")]
    img_type: String,
}

/// Service for interacting with ComfyUI.
pub struct ComfyUiService {
    client: Client,
    base_url: Arc<RwLock<Option<String>>>,
    api_key: Arc<RwLock<Option<String>>>,
    client_id: String,
}

impl ComfyUiService {
    /// Create a new ComfyUI service.
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(600))
                .build()
                .expect("Failed to create HTTP client"),
            base_url: Arc::new(RwLock::new(None)),
            api_key: Arc::new(RwLock::new(None)),
            client_id: Uuid::new_v4().to_string(),
        }
    }

    /// Set the base URL (from SSH tunnel or direct HTTPS).
    pub async fn set_base_url(&self, url: String) {
        let mut base_url = self.base_url.write().await;
        *base_url = Some(url);
    }

    /// Set the API key for authenticated requests.
    pub async fn set_api_key(&self, key: Option<String>) {
        let mut api_key = self.api_key.write().await;
        *api_key = key;
    }

    /// Get the current API key.
    async fn get_api_key(&self) -> Option<String> {
        self.api_key.read().await.clone()
    }

    /// Get the current base URL.
    async fn get_base_url(&self) -> Result<String> {
        let base_url = self.base_url.read().await;
        base_url
            .clone()
            .context("ComfyUI base URL not set - tunnel not established")
    }

    /// Check if connected to ComfyUI.
    pub async fn is_connected(&self) -> bool {
        if let Ok(url) = self.get_base_url().await {
            let mut request = self.client.get(format!("{}/system_stats", url));
            if let Some(key) = self.get_api_key().await {
                request = request.header("X-API-Key", key);
            }
            request
                .send()
                .await
                .map(|r| r.status().is_success())
                .unwrap_or(false)
        } else {
            false
        }
    }

    /// Upload an image to ComfyUI.
    pub async fn upload_image(&self, image_path: &Path) -> Result<String> {
        let base_url = self.get_base_url().await?;

        let file_name = image_path
            .file_name()
            .and_then(|n| n.to_str())
            .context("Invalid image filename")?;

        let file_bytes = std::fs::read(image_path)
            .context(format!("Failed to read image: {:?}", image_path))?;

        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(file_name.to_string())
            .mime_str("image/jpeg")?;

        let form = reqwest::multipart::Form::new()
            .part("image", part)
            .text("overwrite", "true");

        let mut request = self
            .client
            .post(format!("{}/upload/image", base_url))
            .multipart(form);
        if let Some(key) = self.get_api_key().await {
            request = request.header("X-API-Key", key);
        }
        let response = request
            .send()
            .await
            .context("Failed to upload image")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Upload failed: {}", error_text);
        }

        #[derive(Deserialize)]
        struct UploadResponse {
            name: String,
        }

        let upload_resp: UploadResponse = response
            .json()
            .await
            .context("Failed to parse upload response")?;

        Ok(upload_resp.name)
    }

    /// Load a workflow template and substitute parameters.
    fn prepare_workflow(
        &self,
        workflow_json: &str,
        params: &HashMap<String, String>,
    ) -> Result<Value> {
        let mut workflow_str = workflow_json.to_string();

        // Apply parameter substitutions on raw JSON string
        for (key, value) in params {
            // For numeric fields (width, height, denoise, seed), replace "{{key}}" or {{key}} with the raw number
            // For string fields, replace {{key}} with the value (keeping surrounding quotes)
            if key == "width" || key == "height" || key == "denoise" || key == "seed" {
                // Replace quoted placeholder with unquoted number
                let quoted_placeholder = format!("\"{{{{{}}}}}\"", key);
                workflow_str = workflow_str.replace(&quoted_placeholder, value);
            }
            // Always try the unquoted placeholder replacement for string values and numeric fields
            let placeholder = format!("{{{{{}}}}}", key);
            workflow_str = workflow_str.replace(&placeholder, value);
        }

        let workflow: Value = serde_json::from_str(&workflow_str)
            .context("Failed to parse workflow JSON")?;

        Ok(workflow)
    }

    /// Queue a workflow for execution.
    pub async fn queue_prompt(&self, workflow: Value) -> Result<String> {
        let base_url = self.get_base_url().await?;

        let payload = json!({
            "prompt": workflow,
            "client_id": self.client_id
        });

        let mut request = self
            .client
            .post(format!("{}/prompt", base_url))
            .json(&payload);
        if let Some(key) = self.get_api_key().await {
            request = request.header("X-API-Key", key);
        }
        let response = request
            .send()
            .await
            .context("Failed to queue prompt")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Failed to queue prompt: {}", error_text);
        }

        #[derive(Deserialize)]
        struct PromptResponse {
            prompt_id: String,
        }

        let prompt_resp: PromptResponse = response
            .json()
            .await
            .context("Failed to parse prompt response")?;

        Ok(prompt_resp.prompt_id)
    }

    /// Generate an image and return progress updates.
    pub async fn generate(
        &self,
        workflow_json: &str,
        params: HashMap<String, String>,
        output_dir: &Path,
    ) -> Result<(
        tokio::sync::mpsc::Receiver<GenerationProgress>,
        tokio::task::JoinHandle<Result<GenerationResult>>,
    )> {
        let base_url = self.get_base_url().await?;
        let api_key = self.get_api_key().await;
        let workflow = self.prepare_workflow(workflow_json, &params)?;
        let prompt_id = self.queue_prompt(workflow).await?;

        let (tx, rx) = tokio::sync::mpsc::channel(100);
        let client = self.client.clone();
        let output_dir = output_dir.to_path_buf();
        let client_id = self.client_id.clone();
        let base_url_clone = base_url.clone();
        let prompt_id_clone = prompt_id.clone();
        let api_key_clone = api_key.clone();

        let handle = tokio::spawn(async move {
            // Connect to WebSocket
            let ws_url = base_url_clone
                .replace("http://", "ws://")
                .replace("https://", "wss://");

            // Build WebSocket URL with optional API key query param for proxies
            let ws_full_url = if let Some(ref key) = api_key_clone {
                format!("{}/ws?clientId={}&api_key={}", ws_url, client_id, key)
            } else {
                format!("{}/ws?clientId={}", ws_url, client_id)
            };

            let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_full_url)
                .await
                .context("Failed to connect to ComfyUI WebSocket")?;

            let (mut write, mut read) = ws_stream.split();

            let mut result_filename: Option<String> = None;
            let mut logs: Vec<String> = Vec::new();
            let mut current_step = 0u32;
            let mut total_steps = 0u32;
            let mut current_progress = 0.0f32;

            while let Some(msg) = read.next().await {
                let msg = msg.context("WebSocket error")?;

                if let Message::Text(text) = msg {
                    if let Ok(ws_msg) = serde_json::from_str::<WsMessage>(&text) {
                        match ws_msg.msg_type.as_str() {
                            "status" => {
                                if let Some(data) = &ws_msg.data {
                                    if let Some(status) = data.get("status") {
                                        if let Some(queue) = status.get("exec_info").and_then(|e| e.get("queue_remaining")) {
                                            if let Some(remaining) = queue.as_u64() {
                                                logs.push(format!("Queue: {} remaining", remaining));
                                                let _ = tx.send(GenerationProgress {
                                                    status: format!("Queue: {} remaining", remaining),
                                                    progress: current_progress,
                                                    current_step,
                                                    total_steps,
                                                    node_id: None,
                                                    logs: logs.clone(),
                                                }).await;
                                            }
                                        }
                                    }
                                }
                            }
                            "execution_start" => {
                                logs.push("Execution started".to_string());
                                let _ = tx.send(GenerationProgress {
                                    status: "Execution started".to_string(),
                                    progress: 0.0,
                                    current_step: 0,
                                    total_steps: 0,
                                    node_id: None,
                                    logs: logs.clone(),
                                }).await;
                            }
                            "execution_cached" => {
                                if let Some(data) = &ws_msg.data {
                                    if let Some(nodes) = data.get("nodes").and_then(|n| n.as_array()) {
                                        logs.push(format!("Cached {} nodes", nodes.len()));
                                        let _ = tx.send(GenerationProgress {
                                            status: format!("Cached {} nodes", nodes.len()),
                                            progress: current_progress,
                                            current_step,
                                            total_steps,
                                            node_id: None,
                                            logs: logs.clone(),
                                        }).await;
                                    }
                                }
                            }
                            "progress" => {
                                if let Some(data) = ws_msg.data {
                                    if let Ok(progress) = serde_json::from_value::<ProgressData>(data) {
                                        current_step = progress.value;
                                        total_steps = progress.max;
                                        current_progress = if progress.max > 0 {
                                            progress.value as f32 / progress.max as f32
                                        } else {
                                            0.0
                                        };
                                        logs.push(format!("Step {}/{}", progress.value, progress.max));
                                        let _ = tx.send(GenerationProgress {
                                            status: "Generating...".to_string(),
                                            progress: current_progress,
                                            current_step,
                                            total_steps,
                                            node_id: None,
                                            logs: logs.clone(),
                                        }).await;
                                    }
                                }
                            }
                            "executing" => {
                                if let Some(data) = ws_msg.data {
                                    if let Ok(exec_data) = serde_json::from_value::<ExecutingData>(data) {
                                        if exec_data.node.is_none() && exec_data.prompt_id.as_ref() == Some(&prompt_id_clone) {
                                            // Execution complete
                                            logs.push("Execution complete".to_string());
                                            let _ = tx.send(GenerationProgress {
                                                status: "Complete".to_string(),
                                                progress: 1.0,
                                                current_step: 0,
                                                total_steps: 0,
                                                node_id: None,
                                                logs: logs.clone(),
                                            }).await;
                                            break;
                                        } else if let Some(node) = exec_data.node {
                                            logs.push(format!("Executing: {}", node));
                                            let _ = tx.send(GenerationProgress {
                                                status: format!("Processing node: {}", node),
                                                progress: current_progress,
                                                current_step,
                                                total_steps,
                                                node_id: Some(node),
                                                logs: logs.clone(),
                                            }).await;
                                        }
                                    }
                                }
                            }
                            "executed" => {
                                if let Some(data) = ws_msg.data {
                                    if let Ok(exec_data) = serde_json::from_value::<ExecutedData>(data) {
                                        logs.push(format!("Node {} completed", exec_data.node));
                                        if let Some(output) = exec_data.output {
                                            if let Some(images) = output.images {
                                                if let Some(img) = images.first() {
                                                    logs.push(format!("Output: {}", img.filename));
                                                    result_filename = Some(img.filename.clone());
                                                }
                                            }
                                        }
                                        let _ = tx.send(GenerationProgress {
                                            status: format!("Node {} completed", exec_data.node),
                                            progress: current_progress,
                                            current_step,
                                            total_steps,
                                            node_id: None,
                                            logs: logs.clone(),
                                        }).await;
                                    }
                                }
                            }
                            "execution_error" => {
                                if let Some(data) = &ws_msg.data {
                                    let error_msg = data.get("exception_message")
                                        .and_then(|m| m.as_str())
                                        .unwrap_or("Unknown error");
                                    logs.push(format!("ERROR: {}", error_msg));
                                    let _ = tx.send(GenerationProgress {
                                        status: format!("Error: {}", error_msg),
                                        progress: current_progress,
                                        current_step,
                                        total_steps,
                                        node_id: None,
                                        logs: logs.clone(),
                                    }).await;
                                }
                            }
                            other => {
                                // Log unhandled message types for debugging
                                logs.push(format!("[{}]", other));
                                let _ = tx.send(GenerationProgress {
                                    status: format!("Processing: {}", other),
                                    progress: current_progress,
                                    current_step,
                                    total_steps,
                                    node_id: None,
                                    logs: logs.clone(),
                                }).await;
                            }
                        }
                    }
                }
            }

            // Download the result
            let filename = result_filename.context("No output image generated")?;
            let image_url = format!(
                "{}/view?filename={}&type=output",
                base_url_clone, filename
            );

            let mut request = client.get(&image_url);
            if let Some(ref key) = api_key_clone {
                request = request.header("X-API-Key", key);
            }
            let response = request.send().await?;
            let bytes = response.bytes().await?;

            let output_path = output_dir.join(&filename);
            std::fs::write(&output_path, &bytes)?;

            Ok(GenerationResult {
                image_path: output_path,
                prompt_id: prompt_id_clone,
            })
        });

        Ok((rx, handle))
    }

    /// Get available checkpoints (models).
    pub async fn get_checkpoints(&self) -> Result<Vec<String>> {
        let base_url = self.get_base_url().await?;

        let mut request = self
            .client
            .get(format!("{}/object_info/CheckpointLoaderSimple", base_url));
        if let Some(key) = self.get_api_key().await {
            request = request.header("X-API-Key", key);
        }
        let response = request
            .send()
            .await
            .context("Failed to get checkpoints")?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to get checkpoints: {}", response.status());
        }

        let info: Value = response.json().await?;

        // Navigate to the checkpoint list
        let checkpoints = info
            .get("CheckpointLoaderSimple")
            .and_then(|v| v.get("input"))
            .and_then(|v| v.get("required"))
            .and_then(|v| v.get("ckpt_name"))
            .and_then(|v| v.get(0))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        Ok(checkpoints)
    }

    /// Get available LoRAs.
    pub async fn get_loras(&self) -> Result<Vec<String>> {
        let base_url = self.get_base_url().await?;

        let mut request = self
            .client
            .get(format!("{}/object_info/LoraLoader", base_url));
        if let Some(key) = self.get_api_key().await {
            request = request.header("X-API-Key", key);
        }
        let response = request
            .send()
            .await
            .context("Failed to get LoRAs")?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to get LoRAs: {}", response.status());
        }

        let info: Value = response.json().await?;

        let loras = info
            .get("LoraLoader")
            .and_then(|v| v.get("input"))
            .and_then(|v| v.get("required"))
            .and_then(|v| v.get("lora_name"))
            .and_then(|v| v.get(0))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        Ok(loras)
    }

    /// Interrupt current generation.
    pub async fn interrupt(&self) -> Result<()> {
        let base_url = self.get_base_url().await?;

        let mut request = self.client.post(format!("{}/interrupt", base_url));
        if let Some(key) = self.get_api_key().await {
            request = request.header("X-API-Key", key);
        }
        request.send().await.context("Failed to interrupt")?;

        Ok(())
    }

    /// Clear the queue.
    pub async fn clear_queue(&self) -> Result<()> {
        let base_url = self.get_base_url().await?;

        let mut request = self
            .client
            .post(format!("{}/queue", base_url))
            .json(&json!({ "clear": true }));
        if let Some(key) = self.get_api_key().await {
            request = request.header("X-API-Key", key);
        }
        request.send().await.context("Failed to clear queue")?;

        Ok(())
    }
}

impl Default for ComfyUiService {
    fn default() -> Self {
        Self::new()
    }
}
