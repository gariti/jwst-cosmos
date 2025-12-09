//! Configuration management for JWST Cosmos.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Main configuration structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub jwst: JwstConfig,
    #[serde(default)]
    pub remote: RemoteConfig,
    #[serde(default)]
    pub generation: GenerationConfig,
    #[serde(default)]
    pub wallust: WallustConfig,
}

/// JWST image source configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwstConfig {
    /// Path to API key file
    #[serde(default = "default_api_key_file")]
    pub api_key_file: String,

    /// Directory for downloaded wallpapers
    #[serde(default = "default_wallpaper_dir")]
    pub wallpaper_dir: String,

    /// Cache directory
    #[serde(default = "default_cache_dir")]
    pub cache_dir: String,

    /// Cache TTL in seconds
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl: u64,

    /// ESA RSS feed URL
    #[serde(default = "default_esa_rss_url")]
    pub esa_rss_url: String,

    /// ESA CDN base URL
    #[serde(default = "default_esa_cdn_base")]
    pub esa_cdn_base: String,

    /// JWST API base URL
    #[serde(default = "default_api_base")]
    pub api_base: String,
}

/// Remote server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteConfig {
    /// Remote host address
    #[serde(default = "default_remote_host")]
    pub host: String,

    /// SSH user
    #[serde(default = "default_remote_user")]
    pub user: String,

    /// Ollama port
    #[serde(default = "default_ollama_port")]
    pub ollama_port: u16,

    /// ComfyUI port
    #[serde(default = "default_comfyui_port")]
    pub comfyui_port: u16,

    /// SSH key path (optional)
    pub ssh_key: Option<String>,
}

/// Image generation configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationConfig {
    /// Default output size
    #[serde(default = "default_size")]
    pub default_size: String,

    /// Default model
    #[serde(default = "default_model")]
    pub default_model: String,

    /// Enable AI upscaling
    #[serde(default = "default_enable_upscaling")]
    pub enable_upscaling: bool,

    /// Upscale model name
    #[serde(default = "default_upscale_model")]
    pub upscale_model: String,
}

/// Wallust color system configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WallustConfig {
    /// Auto-apply generated images as wallpaper
    #[serde(default = "default_auto_apply")]
    pub auto_apply: bool,

    /// Refresh theme script path
    #[serde(default = "default_refresh_script")]
    pub refresh_script: String,

    /// Color scheme file path
    #[serde(default = "default_color_scheme_path")]
    pub color_scheme_path: String,
}

// Default value functions
fn default_api_key_file() -> String {
    "/run/agenix/jwst-api-key".to_string()
}

fn default_wallpaper_dir() -> String {
    "~/Pictures/Wallpapers".to_string()
}

fn default_cache_dir() -> String {
    "~/.cache/jwst-cosmos".to_string()
}

fn default_cache_ttl() -> u64 {
    3600
}

fn default_esa_rss_url() -> String {
    "https://feeds.feedburner.com/esawebb/images/".to_string()
}

fn default_esa_cdn_base() -> String {
    "https://cdn.esawebb.org/archives/images".to_string()
}

fn default_api_base() -> String {
    "https://api.jwstapi.com".to_string()
}

fn default_remote_host() -> String {
    "192.168.0.27".to_string()
}

fn default_remote_user() -> String {
    "garrett".to_string()
}

fn default_ollama_port() -> u16 {
    11434
}

fn default_comfyui_port() -> u16 {
    8188
}

fn default_size() -> String {
    "5120x2160".to_string()
}

fn default_model() -> String {
    "sdxl".to_string()
}

fn default_enable_upscaling() -> bool {
    true
}

fn default_upscale_model() -> String {
    "realesrgan-x4plus".to_string()
}

fn default_auto_apply() -> bool {
    true
}

fn default_refresh_script() -> String {
    "/etc/nixos/scripts/refresh-theme".to_string()
}

fn default_color_scheme_path() -> String {
    "~/.local/state/caelestia/scheme/current.txt".to_string()
}

impl Default for JwstConfig {
    fn default() -> Self {
        Self {
            api_key_file: default_api_key_file(),
            wallpaper_dir: default_wallpaper_dir(),
            cache_dir: default_cache_dir(),
            cache_ttl: default_cache_ttl(),
            esa_rss_url: default_esa_rss_url(),
            esa_cdn_base: default_esa_cdn_base(),
            api_base: default_api_base(),
        }
    }
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            host: default_remote_host(),
            user: default_remote_user(),
            ollama_port: default_ollama_port(),
            comfyui_port: default_comfyui_port(),
            ssh_key: None,
        }
    }
}

impl Default for GenerationConfig {
    fn default() -> Self {
        Self {
            default_size: default_size(),
            default_model: default_model(),
            enable_upscaling: default_enable_upscaling(),
            upscale_model: default_upscale_model(),
        }
    }
}

impl Default for WallustConfig {
    fn default() -> Self {
        Self {
            auto_apply: default_auto_apply(),
            refresh_script: default_refresh_script(),
            color_scheme_path: default_color_scheme_path(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            jwst: JwstConfig::default(),
            remote: RemoteConfig::default(),
            generation: GenerationConfig::default(),
            wallust: WallustConfig::default(),
        }
    }
}

impl Config {
    /// Get the default config file path.
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("jwst-cosmos")
            .join("config.toml")
    }

    /// Load configuration from the default location.
    pub fn load() -> Result<Self> {
        let path = Self::config_path();
        if path.exists() {
            Self::from_file(path.to_str().unwrap())
        } else {
            Ok(Self::default())
        }
    }

    /// Load configuration from a specific file.
    pub fn from_file(path: &str) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path))?;
        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path))?;
        Ok(config)
    }

    /// Expand ~ in paths to home directory.
    pub fn expand_path(&self, path: &str) -> PathBuf {
        if path.starts_with("~/") {
            if let Some(home) = dirs::home_dir() {
                return home.join(&path[2..]);
            }
        }
        PathBuf::from(path)
    }

    /// Get the wallpaper directory path.
    pub fn wallpaper_dir(&self) -> PathBuf {
        self.expand_path(&self.jwst.wallpaper_dir)
    }

    /// Get the cache directory path.
    pub fn cache_dir(&self) -> PathBuf {
        self.expand_path(&self.jwst.cache_dir)
    }

    /// Get the thumbnail cache directory.
    pub fn thumbnail_dir(&self) -> PathBuf {
        self.cache_dir().join("thumbnails")
    }

    /// Read the JWST API key from file.
    pub fn jwst_api_key(&self) -> Option<String> {
        fs::read_to_string(&self.jwst.api_key_file)
            .ok()
            .map(|s| s.trim().to_string())
    }

    /// Parse size string like "5120x2160" into (width, height).
    pub fn parse_size(&self, size_str: &str) -> (u32, u32) {
        // Check presets
        let size = match size_str.to_lowercase().as_str() {
            "hd" => "1920x1080",
            "qhd" => "2560x1440",
            "laptop" => "2560x1600",
            "4k" => "3840x2160",
            "ultrawide" => "5120x2160",
            _ => size_str,
        };

        size.split('x')
            .map(|s| s.parse::<u32>().unwrap_or(0))
            .collect::<Vec<_>>()
            .get(0..2)
            .map(|v| (v[0], v[1]))
            .unwrap_or((5120, 2160))
    }
}
