//! JWST API service - fetches raw scientific images from jwstapi.com.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use crate::config::Config;

/// Represents an image from the JWST API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwstImage {
    pub id: String,
    #[serde(default)]
    pub observation_id: Option<String>,
    #[serde(default)]
    pub program: Option<i32>,
    #[serde(default)]
    pub details: Option<ImageDetails>,
    #[serde(default)]
    pub file_type: Option<String>,
    #[serde(default)]
    pub thumbnail: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageDetails {
    #[serde(default)]
    pub mission: Option<String>,
    #[serde(default)]
    pub instruments: Option<Vec<String>>,
    #[serde(default)]
    pub suffix: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

impl JwstImage {
    /// Get a display title for the image.
    pub fn title(&self) -> String {
        if let Some(details) = &self.details {
            if let Some(desc) = &details.description {
                if !desc.is_empty() {
                    return desc.clone();
                }
            }
        }
        self.observation_id
            .clone()
            .unwrap_or_else(|| self.id.clone())
    }

    /// Get instruments as a comma-separated string.
    pub fn instruments_str(&self) -> String {
        if let Some(details) = &self.details {
            if let Some(instruments) = &details.instruments {
                return instruments.join(", ");
            }
        }
        String::new()
    }
}

/// API response structure.
#[derive(Debug, Deserialize)]
struct ApiResponse {
    #[serde(default)]
    body: Vec<JwstImage>,
}

/// Service for fetching images from JWST API.
pub struct JwstApiService {
    config: Arc<Config>,
    client: Client,
    cache_file: PathBuf,
    api_key: Option<String>,
}

impl JwstApiService {
    /// Create a new JWST API service.
    pub fn new(config: Arc<Config>) -> Self {
        let cache_dir = config.cache_dir();
        fs::create_dir_all(&cache_dir).ok();

        let api_key = config.jwst_api_key();

        Self {
            cache_file: cache_dir.join("jwst_api_metadata.json"),
            config,
            client: Client::builder()
                .user_agent("JWST-Cosmos/0.1.0 (Rust; Ratatui TUI)")
                .build()
                .expect("Failed to create HTTP client"),
            api_key,
        }
    }

    /// Check if we have a valid API key.
    pub fn has_api_key(&self) -> bool {
        self.api_key.is_some()
    }

    /// Check if the cache is still valid.
    fn is_cache_valid(&self) -> bool {
        if !self.cache_file.exists() {
            return false;
        }

        if let Ok(metadata) = fs::metadata(&self.cache_file) {
            if let Ok(modified) = metadata.modified() {
                if let Ok(duration) = SystemTime::now().duration_since(modified) {
                    return duration.as_secs() < self.config.jwst.cache_ttl;
                }
            }
        }

        false
    }

    /// Fetch images from the API.
    async fn fetch_images(&self, force_refresh: bool) -> Result<Vec<JwstImage>> {
        // Check cache first
        if !force_refresh && self.is_cache_valid() {
            let content = fs::read_to_string(&self.cache_file)
                .context("Failed to read API cache")?;
            let images: Vec<JwstImage> = serde_json::from_str(&content)
                .context("Failed to parse cached images")?;
            return Ok(images);
        }

        // Need API key for fresh fetch
        let api_key = self
            .api_key
            .as_ref()
            .context("No JWST API key available")?;

        // Fetch from API
        let url = format!("{}/all/type/jpg", self.config.jwst.api_base);
        let response = self
            .client
            .get(&url)
            .header("X-API-KEY", api_key)
            .send()
            .await
            .context("Failed to fetch from JWST API")?;

        if !response.status().is_success() {
            anyhow::bail!("API request failed with status: {}", response.status());
        }

        let api_response: ApiResponse = response
            .json()
            .await
            .context("Failed to parse API response")?;

        // Cache the response
        let cache_content = serde_json::to_string_pretty(&api_response.body)?;
        fs::write(&self.cache_file, &cache_content).ok();

        Ok(api_response.body)
    }

    /// Get list of available images.
    pub async fn get_images(&self, force_refresh: bool) -> Result<Vec<JwstImage>> {
        self.fetch_images(force_refresh).await
    }

    /// Download an image.
    pub async fn download_image(&self, image: &JwstImage) -> Result<PathBuf> {
        let wallpaper_dir = self.config.wallpaper_dir();
        fs::create_dir_all(&wallpaper_dir)?;

        let output_path = wallpaper_dir.join(format!("jwst-{}.jpg", image.id));

        // Return cached if exists
        if output_path.exists() {
            return Ok(output_path);
        }

        // Get download URL
        let url = image
            .location
            .as_ref()
            .context("No download URL available for image")?;

        // Download
        let response = self
            .client
            .get(url)
            .send()
            .await
            .context("Failed to download image")?;

        if !response.status().is_success() {
            anyhow::bail!("Download failed with status: {}", response.status());
        }

        let bytes = response.bytes().await?;
        fs::write(&output_path, &bytes)?;

        Ok(output_path)
    }

    /// Download a thumbnail for an image.
    pub async fn download_thumbnail(&self, image: &JwstImage) -> Result<PathBuf> {
        let thumbnail_dir = self.config.thumbnail_dir();
        fs::create_dir_all(&thumbnail_dir)?;

        let thumbnail_path = thumbnail_dir.join(format!("{}.thumb.jpg", image.id));

        // Return cached if exists
        if thumbnail_path.exists() {
            return Ok(thumbnail_path);
        }

        // Get thumbnail URL
        let url = image
            .thumbnail
            .as_ref()
            .context("No thumbnail URL available")?;

        // Download
        let response = self.client.get(url).send().await?;

        if response.status().is_success() {
            let bytes = response.bytes().await?;
            fs::write(&thumbnail_path, &bytes)?;
        }

        Ok(thumbnail_path)
    }

    /// Check if an image is already downloaded.
    pub fn is_downloaded(&self, image: &JwstImage) -> bool {
        let wallpaper_dir = self.config.wallpaper_dir();
        wallpaper_dir.join(format!("jwst-{}.jpg", image.id)).exists()
    }
}
