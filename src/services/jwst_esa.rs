//! ESA/Webb image service - fetches colorized JWST images from ESA's RSS feed.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use quick_xml::de::from_str;
use reqwest::Client;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use crate::config::Config;

/// Represents an image from ESA/Webb gallery.
#[derive(Debug, Clone)]
pub struct EsaImage {
    pub id: String,
    pub title: String,
    pub pub_date: Option<DateTime<Utc>>,
    pub enclosure_url: Option<String>,
}

impl EsaImage {
    /// Get the thumbnail URL.
    pub fn thumbnail_url(&self, config: &Config) -> String {
        format!("{}/news/{}.jpg", config.jwst.esa_cdn_base, self.id)
    }

    /// Get the screen-size URL.
    pub fn screen_url(&self, config: &Config) -> String {
        format!("{}/screen/{}.jpg", config.jwst.esa_cdn_base, self.id)
    }

    /// Get the UHD wallpaper URL.
    pub fn wallpaper_uhd_url(&self, config: &Config) -> String {
        format!("{}/wallpaper_uhd/{}.jpg", config.jwst.esa_cdn_base, self.id)
    }

    /// Get the large version URL.
    pub fn large_url(&self, config: &Config) -> String {
        format!("{}/large/{}.jpg", config.jwst.esa_cdn_base, self.id)
    }

    /// Get the ESA gallery page URL.
    pub fn gallery_url(&self) -> String {
        format!("https://esawebb.org/images/{}/", self.id)
    }
}

/// RSS feed structures for deserialization.
#[derive(Debug, Deserialize)]
struct Rss {
    channel: Channel,
}

#[derive(Debug, Deserialize)]
struct Channel {
    #[serde(rename = "item", default)]
    items: Vec<RssItem>,
}

#[derive(Debug, Deserialize)]
struct RssItem {
    title: Option<String>,
    guid: Option<String>,
    #[serde(rename = "pubDate")]
    pub_date: Option<String>,
    enclosure: Option<Enclosure>,
}

#[derive(Debug, Deserialize)]
struct Enclosure {
    url: Option<String>,
}

/// Service for fetching images from ESA/Webb RSS feed.
pub struct EsaService {
    config: Arc<Config>,
    client: Client,
    cache_file: PathBuf,
}

impl EsaService {
    /// Create a new ESA service.
    pub fn new(config: Arc<Config>) -> Self {
        let cache_dir = config.cache_dir();
        fs::create_dir_all(&cache_dir).ok();

        Self {
            cache_file: cache_dir.join("esa_metadata.xml"),
            config,
            client: Client::builder()
                .user_agent("JWST-Cosmos/0.1.0 (Rust; Ratatui TUI)")
                .build()
                .expect("Failed to create HTTP client"),
        }
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

    /// Fetch the RSS feed.
    async fn fetch_rss(&self, force_refresh: bool) -> Result<String> {
        // Check cache first
        if !force_refresh && self.is_cache_valid() {
            return fs::read_to_string(&self.cache_file)
                .context("Failed to read RSS cache");
        }

        // Fetch from RSS
        let response = self
            .client
            .get(&self.config.jwst.esa_rss_url)
            .send()
            .await
            .context("Failed to fetch ESA RSS feed")?;

        let content = response
            .text()
            .await
            .context("Failed to read RSS response")?;

        // Cache the response
        fs::write(&self.cache_file, &content).ok();

        Ok(content)
    }

    /// Parse RSS content into images.
    fn parse_rss(&self, content: &str) -> Result<Vec<EsaImage>> {
        let rss: Rss = from_str(content).context("Failed to parse RSS XML")?;
        let mut images = Vec::new();

        for item in rss.channel.items {
            // Extract ID from guid URL
            let id = if let Some(guid) = &item.guid {
                // e.g., https://esawebb.org/images/potm2511a/ -> potm2511a
                guid.trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .to_string()
            } else {
                continue;
            };

            if id.is_empty() {
                continue;
            }

            let title = item
                .title
                .unwrap_or_else(|| id.clone())
                .replace("&amp;", "&")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"");

            let pub_date = item.pub_date.and_then(|d| {
                // Parse RSS date format: "Mon, 01 Jan 2024 00:00:00 +0000"
                DateTime::parse_from_rfc2822(&d)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
            });

            let enclosure_url = item.enclosure.and_then(|e| e.url);

            images.push(EsaImage {
                id,
                title,
                pub_date,
                enclosure_url,
            });
        }

        // Sort by publication date (newest first)
        images.sort_by(|a, b| b.pub_date.cmp(&a.pub_date));

        Ok(images)
    }

    /// Get list of available images.
    pub async fn get_images(&self, force_refresh: bool) -> Result<Vec<EsaImage>> {
        let content = self.fetch_rss(force_refresh).await?;
        self.parse_rss(&content)
    }

    /// Download a thumbnail for an image.
    pub async fn download_thumbnail(&self, image: &EsaImage) -> Result<PathBuf> {
        let thumbnail_dir = self.config.thumbnail_dir();
        fs::create_dir_all(&thumbnail_dir)?;

        let thumbnail_path = thumbnail_dir.join(format!("{}.thumb.jpg", image.id));

        // Return cached if exists
        if thumbnail_path.exists() {
            return Ok(thumbnail_path);
        }

        // Try news-sized thumbnail first
        let url = image.thumbnail_url(&self.config);
        let response = self.client.get(&url).send().await;

        if let Ok(resp) = response {
            if resp.status().is_success() {
                let bytes = resp.bytes().await?;
                fs::write(&thumbnail_path, &bytes)?;
                return Ok(thumbnail_path);
            }
        }

        // Fallback to screen size
        let url = image.screen_url(&self.config);
        let response = self.client.get(&url).send().await?;
        let bytes = response.bytes().await?;
        fs::write(&thumbnail_path, &bytes)?;

        Ok(thumbnail_path)
    }

    /// Download an image at the specified resolution.
    pub async fn download_image(
        &self,
        image: &EsaImage,
        resolution: &str,
    ) -> Result<PathBuf> {
        let wallpaper_dir = self.config.wallpaper_dir();
        fs::create_dir_all(&wallpaper_dir)?;

        let output_path = wallpaper_dir.join(format!("webb-{}.jpg", image.id));

        // Select URL based on resolution
        let url = match resolution {
            "thumbnail" => image.thumbnail_url(&self.config),
            "screen" => image.screen_url(&self.config),
            "large" => image.large_url(&self.config),
            _ => image.wallpaper_uhd_url(&self.config),
        };

        // Try downloading
        let try_download = async |url: &str| -> Result<Vec<u8>> {
            let response = self.client.get(url).send().await?;
            if response.status().is_success() {
                Ok(response.bytes().await?.to_vec())
            } else {
                anyhow::bail!("Download failed with status: {}", response.status())
            }
        };

        // Try requested resolution, then fallbacks
        let bytes = match try_download(&url).await {
            Ok(b) => b,
            Err(_) => match try_download(&image.large_url(&self.config)).await {
                Ok(b) => b,
                Err(_) => try_download(&image.screen_url(&self.config)).await?,
            },
        };

        fs::write(&output_path, &bytes)?;
        Ok(output_path)
    }

    /// Check if an image is already downloaded.
    pub fn is_downloaded(&self, image: &EsaImage) -> bool {
        let wallpaper_dir = self.config.wallpaper_dir();
        wallpaper_dir.join(format!("webb-{}.jpg", image.id)).exists()
            || wallpaper_dir.join(format!("webb-{}-ultrawide.jpg", image.id)).exists()
    }

    /// Get path to downloaded image if it exists.
    pub fn get_downloaded_path(&self, image: &EsaImage) -> Option<PathBuf> {
        let wallpaper_dir = self.config.wallpaper_dir();
        let paths = [
            wallpaper_dir.join(format!("webb-{}-ultrawide.jpg", image.id)),
            wallpaper_dir.join(format!("webb-{}.jpg", image.id)),
            wallpaper_dir.join(format!("webb-{}-laptop.jpg", image.id)),
        ];

        paths.into_iter().find(|p| p.exists())
    }
}
