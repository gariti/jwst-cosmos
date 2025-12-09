//! Wallust color system integration.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::config::Config;

/// Parsed wallust colors.
#[derive(Debug, Clone, Default)]
pub struct WallustColors {
    pub background: String,
    pub foreground: String,
    pub cursor: String,
    pub colors: Vec<String>,  // 0-15
    pub color_map: HashMap<String, String>,
}

impl WallustColors {
    /// Get a specific color by index.
    pub fn color(&self, index: usize) -> Option<&String> {
        self.colors.get(index)
    }

    /// Get color by name (e.g., "color0", "background").
    pub fn get(&self, name: &str) -> Option<&String> {
        self.color_map.get(name)
    }

    /// Get primary accent color (usually color4 or color6).
    pub fn accent(&self) -> &str {
        self.colors.get(4).map(|s| s.as_str()).unwrap_or("#89b4fa")
    }

    /// Get secondary accent color.
    pub fn accent_secondary(&self) -> &str {
        self.colors.get(6).map(|s| s.as_str()).unwrap_or("#94e2d5")
    }

    /// Get warning/error color (usually color1).
    pub fn error(&self) -> &str {
        self.colors.get(1).map(|s| s.as_str()).unwrap_or("#f38ba8")
    }

    /// Get success color (usually color2).
    pub fn success(&self) -> &str {
        self.colors.get(2).map(|s| s.as_str()).unwrap_or("#a6e3a1")
    }

    /// Get muted/dim color (usually color8).
    pub fn muted(&self) -> &str {
        self.colors.get(8).map(|s| s.as_str()).unwrap_or("#6c7086")
    }
}

/// Service for wallust integration.
pub struct WallustService {
    config: Config,
}

impl WallustService {
    /// Create a new wallust service.
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Load current wallust colors.
    pub fn load_colors(&self) -> Result<WallustColors> {
        // Try to load from the standard wallust colors file
        let colors_path = dirs::cache_dir()
            .unwrap_or_default()
            .join("wallust/colors.sh");

        if !colors_path.exists() {
            // Return default catppuccin-mocha-like colors
            return Ok(self.default_colors());
        }

        let content = fs::read_to_string(&colors_path)
            .context("Failed to read wallust colors")?;

        self.parse_colors_sh(&content)
    }

    /// Parse colors from wallust colors.sh format.
    fn parse_colors_sh(&self, content: &str) -> Result<WallustColors> {
        let mut colors = WallustColors::default();
        colors.colors = vec![String::new(); 16];

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Parse lines like: export color0='#1e1e2e'
            // or: wallust_color0='#1e1e2e'
            if let Some(eq_pos) = line.find('=') {
                let key = line[..eq_pos]
                    .trim()
                    .trim_start_matches("export ")
                    .trim_start_matches("wallust_");
                let value = line[eq_pos + 1..]
                    .trim()
                    .trim_matches(|c| c == '\'' || c == '"');

                colors.color_map.insert(key.to_string(), value.to_string());

                match key {
                    "background" => colors.background = value.to_string(),
                    "foreground" => colors.foreground = value.to_string(),
                    "cursor" => colors.cursor = value.to_string(),
                    _ if key.starts_with("color") => {
                        if let Ok(idx) = key[5..].parse::<usize>() {
                            if idx < 16 {
                                colors.colors[idx] = value.to_string();
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Fill in missing colors with defaults
        if colors.background.is_empty() {
            colors.background = colors.colors.get(0).cloned().unwrap_or_else(|| "#1e1e2e".to_string());
        }
        if colors.foreground.is_empty() {
            colors.foreground = colors.colors.get(7).cloned().unwrap_or_else(|| "#cdd6f4".to_string());
        }
        if colors.cursor.is_empty() {
            colors.cursor = colors.foreground.clone();
        }

        Ok(colors)
    }

    /// Get default colors (Catppuccin Mocha-like).
    fn default_colors(&self) -> WallustColors {
        WallustColors {
            background: "#1e1e2e".to_string(),
            foreground: "#cdd6f4".to_string(),
            cursor: "#f5e0dc".to_string(),
            colors: vec![
                "#45475a".to_string(),  // 0: black
                "#f38ba8".to_string(),  // 1: red
                "#a6e3a1".to_string(),  // 2: green
                "#f9e2af".to_string(),  // 3: yellow
                "#89b4fa".to_string(),  // 4: blue
                "#f5c2e7".to_string(),  // 5: magenta
                "#94e2d5".to_string(),  // 6: cyan
                "#bac2de".to_string(),  // 7: white
                "#585b70".to_string(),  // 8: bright black
                "#f38ba8".to_string(),  // 9: bright red
                "#a6e3a1".to_string(),  // 10: bright green
                "#f9e2af".to_string(),  // 11: bright yellow
                "#89b4fa".to_string(),  // 12: bright blue
                "#f5c2e7".to_string(),  // 13: bright magenta
                "#94e2d5".to_string(),  // 14: bright cyan
                "#a6adc8".to_string(),  // 15: bright white
            ],
            color_map: HashMap::new(),
        }
    }

    /// Apply a wallpaper and refresh the theme.
    /// Runs in background to avoid blocking the TUI.
    pub fn apply_wallpaper(&self, image_path: &Path) -> Result<()> {
        let refresh_script = &self.config.wallust.refresh_script;

        if !Path::new(refresh_script).exists() {
            anyhow::bail!("Refresh script not found: {}", refresh_script);
        }

        // Spawn detached process that won't interfere with TUI
        // Redirect stdout/stderr to /dev/null so it doesn't mess up the terminal
        Command::new(refresh_script)
            .arg(image_path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("Failed to spawn refresh script")?;

        // Don't wait for it - let it run in background
        Ok(())
    }

    /// Run wallust to generate colors from an image.
    pub fn generate_colors(&self, image_path: &Path) -> Result<()> {
        let status = Command::new("wallust")
            .arg("run")
            .arg(image_path)
            .status()
            .context("Failed to run wallust")?;

        if !status.success() {
            anyhow::bail!("Wallust failed with code: {:?}", status.code());
        }

        Ok(())
    }

    /// Get the current wallpaper path.
    pub fn current_wallpaper(&self) -> Option<String> {
        let cache_file = dirs::cache_dir()?.join("wallust/current-wallpaper");
        fs::read_to_string(&cache_file).ok().map(|s| s.trim().to_string())
    }

    /// Get the current color scheme name.
    pub fn current_scheme(&self) -> Option<String> {
        let scheme_path = self.config.expand_path(&self.config.wallust.color_scheme_path);
        fs::read_to_string(&scheme_path).ok().map(|s| s.trim().to_string())
    }
}
