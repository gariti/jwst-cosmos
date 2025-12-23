//! Image generation screen.

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::Screen;
use crate::config::Config;
use crate::services::{ComfyUiService, GenerationProgress, GenerationResult, OllamaService, WallustService};
use crate::utils::SizePreset;

/// Generation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationMode {
    Img2Img,
    ControlNetDepth,
    ControlNetCanny,
}

impl GenerationMode {
    fn name(&self) -> &str {
        match self {
            Self::Img2Img => "img2img",
            Self::ControlNetDepth => "ControlNet Depth",
            Self::ControlNetCanny => "ControlNet Canny",
        }
    }

    fn all() -> Vec<Self> {
        vec![Self::Img2Img, Self::ControlNetDepth, Self::ControlNetCanny]
    }
}

/// Focus state for the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FormFocus {
    Mode,
    Size,
    Model,
    Denoise,
    Prompt,
    Generate,
}

/// Generator screen state.
pub struct GeneratorScreen {
    comfyui_service: Arc<ComfyUiService>,
    ollama_service: Arc<OllamaService>,
    wallust_service: Arc<WallustService>,
    config: Arc<Config>,

    // Form state
    focus: FormFocus,
    mode: GenerationMode,
    mode_idx: usize,
    size: SizePreset,
    size_idx: usize,
    prompt: String,
    model: String,
    available_models: Vec<String>,
    model_idx: usize,
    denoise: f32,

    // Reference image
    reference_image: Option<String>,

    // Generation state
    generating: bool,
    progress: Option<GenerationProgress>,
    result_path: Option<String>,
    error: Option<String>,

    // Async generation handles
    progress_rx: Option<mpsc::Receiver<GenerationProgress>>,
    generation_handle: Option<JoinHandle<anyhow::Result<GenerationResult>>>,
    output_dir: Option<PathBuf>,

    // Model loading state
    models_loaded: bool,
}

impl GeneratorScreen {
    pub fn new(
        comfyui_service: Arc<ComfyUiService>,
        ollama_service: Arc<OllamaService>,
        wallust_service: Arc<WallustService>,
        config: Arc<Config>,
    ) -> Self {
        Self {
            comfyui_service,
            ollama_service,
            wallust_service,
            config,
            focus: FormFocus::Mode,
            mode: GenerationMode::Img2Img,
            mode_idx: 0,
            size: SizePreset::Ultrawide,
            size_idx: 4, // Ultrawide is index 4
            prompt: String::new(),
            model: String::new(),
            available_models: Vec::new(),
            model_idx: 0,
            denoise: 0.3,
            reference_image: None,
            generating: false,
            progress: None,
            result_path: None,
            error: None,
            progress_rx: None,
            generation_handle: None,
            output_dir: None,
            models_loaded: false,
        }
    }

    /// Set the reference image from browser.
    pub fn set_reference_image(&mut self, path: String) {
        self.reference_image = Some(path);
    }

    /// Check if currently editing the prompt field.
    pub fn is_editing_prompt(&self) -> bool {
        self.focus == FormFocus::Prompt
    }

    /// Exit prompt editing mode (move focus away from prompt).
    pub fn exit_prompt_editing(&mut self) {
        if self.focus == FormFocus::Prompt {
            self.focus = FormFocus::Generate;
        }
    }

    /// Check if currently generating.
    pub fn is_generating(&self) -> bool {
        self.generating
    }

    /// Load available models from ComfyUI.
    pub async fn load_models(&mut self) {
        if let Ok(models) = self.comfyui_service.get_checkpoints().await {
            if !models.is_empty() {
                self.available_models = models;
                // Keep current selection if valid, otherwise reset to first
                if self.model_idx >= self.available_models.len() {
                    self.model_idx = 0;
                }
                self.model = self.available_models[self.model_idx].clone();
                self.models_loaded = true;
            }
        }
    }

    /// Ensure models are loaded (call when switching to this screen).
    pub async fn ensure_models_loaded(&mut self) {
        if !self.models_loaded {
            self.load_models().await;
        }
    }

    /// Start generation (non-blocking - kicks off the task and returns).
    async fn start_generation(&mut self) {
        // Check if ComfyUI is connected first
        if !self.comfyui_service.is_connected().await {
            self.error = Some("Not connected to ComfyUI. Go to Services tab to connect.".to_string());
            self.generating = false;
            self.progress = None;
            return;
        }

        if self.reference_image.is_none() {
            self.error = Some("No reference image selected".to_string());
            return;
        }

        if self.model.is_empty() {
            self.error = Some("No model selected. Press [r] to refresh models.".to_string());
            return;
        }

        self.generating = true;
        self.error = None;
        self.result_path = None;
        self.progress = Some(GenerationProgress {
            status: "Starting...".to_string(),
            progress: 0.0,
            current_step: 0,
            total_steps: 0,
            node_id: None,
            logs: Vec::new(),
        });

        // Build generation parameters
        let (width, height) = self.size.dimensions();
        let seed: u64 = rand::random();
        let mut params = HashMap::new();
        params.insert("width".to_string(), width.to_string());
        params.insert("height".to_string(), height.to_string());
        params.insert("prompt".to_string(), self.prompt.clone());
        params.insert("model".to_string(), self.model.clone());
        params.insert("denoise".to_string(), format!("{:.2}", self.denoise));
        params.insert("seed".to_string(), seed.to_string());
        params.insert(
            "image".to_string(),
            self.reference_image.clone().unwrap_or_default(),
        );

        // Select workflow based on mode
        let workflow = match self.mode {
            GenerationMode::Img2Img => include_str!("../../workflows/img2img_sdxl.json"),
            GenerationMode::ControlNetDepth => include_str!("../../workflows/controlnet_depth.json"),
            GenerationMode::ControlNetCanny => include_str!("../../workflows/controlnet_canny.json"),
        };

        let output_dir = self.config.wallpaper_dir();

        // Ensure output directory exists
        if let Err(e) = std::fs::create_dir_all(&output_dir) {
            self.error = Some(format!("Failed to create output directory: {}", e));
            self.generating = false;
            return;
        }

        // Upload the reference image first
        let ref_path = std::path::Path::new(self.reference_image.as_ref().unwrap());
        self.progress = Some(GenerationProgress {
            status: "Uploading reference image...".to_string(),
            progress: 0.0,
            current_step: 0,
            total_steps: 0,
            node_id: None,
            logs: vec!["Uploading reference image...".to_string()],
        });

        let uploaded_name = match self.comfyui_service.upload_image(ref_path).await {
            Ok(name) => name,
            Err(e) => {
                self.error = Some(format!("Failed to upload image: {}", e));
                self.generating = false;
                return;
            }
        };

        // Update progress with upload success
        self.progress = Some(GenerationProgress {
            status: "Starting generation...".to_string(),
            progress: 0.0,
            current_step: 0,
            total_steps: 0,
            node_id: None,
            logs: vec!["Uploading reference image...".to_string(), format!("Uploaded as: {}", uploaded_name), "Starting generation...".to_string()],
        });

        // Update params with uploaded image name
        params.insert("image".to_string(), uploaded_name);

        // Start generation and store handles (non-blocking)
        match self.comfyui_service.generate(workflow, params, &output_dir).await {
            Ok((rx, handle)) => {
                self.progress_rx = Some(rx);
                self.generation_handle = Some(handle);
                self.output_dir = Some(output_dir);
                // Generation is now running in the background
                // poll_progress() will handle updates
            }
            Err(e) => {
                self.error = Some(format!("Failed to start generation: {}", e));
                self.generating = false;
            }
        }
    }

    /// Poll for progress updates (call this from the event loop).
    /// Returns true if still generating, false if complete.
    pub async fn poll_progress(&mut self) -> bool {
        if !self.generating {
            return false;
        }

        // Check for progress updates (non-blocking)
        if let Some(ref mut rx) = self.progress_rx {
            // Try to receive all available progress updates
            loop {
                match rx.try_recv() {
                    Ok(prog) => {
                        let is_complete = prog.status == "Complete";
                        self.progress = Some(prog);
                        if is_complete {
                            break;
                        }
                    }
                    Err(mpsc::error::TryRecvError::Empty) => {
                        // No more updates available right now
                        break;
                    }
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        // Channel closed, generation task finished
                        break;
                    }
                }
            }
        }

        // Check if generation task is complete
        if let Some(ref handle) = self.generation_handle {
            if handle.is_finished() {
                // Take ownership of the handle to await it
                if let Some(handle) = self.generation_handle.take() {
                    match handle.await {
                        Ok(Ok(result)) => {
                            self.result_path = Some(result.image_path.to_string_lossy().to_string());
                            self.progress = None;

                            // Auto-apply wallpaper if configured
                            if self.config.wallust.auto_apply {
                                if let Err(e) = self.wallust_service.apply_wallpaper(&result.image_path) {
                                    self.error = Some(format!("Generated but failed to apply: {}", e));
                                }
                            }
                        }
                        Ok(Err(e)) => {
                            self.error = Some(format!("Generation failed: {}", e));
                        }
                        Err(e) => {
                            self.error = Some(format!("Generation task panicked: {}", e));
                        }
                    }
                }

                // Clean up
                self.progress_rx = None;
                self.output_dir = None;
                self.generating = false;
                return false;
            }
        }

        true
    }

    /// Navigate to next form field.
    fn next_field(&mut self) {
        self.focus = match self.focus {
            FormFocus::Mode => FormFocus::Size,
            FormFocus::Size => FormFocus::Model,
            FormFocus::Model => FormFocus::Denoise,
            FormFocus::Denoise => FormFocus::Prompt,
            FormFocus::Prompt => FormFocus::Generate,
            FormFocus::Generate => FormFocus::Mode,
        };
    }

    /// Navigate to previous form field.
    fn prev_field(&mut self) {
        self.focus = match self.focus {
            FormFocus::Mode => FormFocus::Generate,
            FormFocus::Size => FormFocus::Mode,
            FormFocus::Model => FormFocus::Size,
            FormFocus::Denoise => FormFocus::Model,
            FormFocus::Prompt => FormFocus::Denoise,
            FormFocus::Generate => FormFocus::Prompt,
        };
    }

    /// Cycle current selection.
    fn cycle_selection(&mut self, forward: bool) {
        match self.focus {
            FormFocus::Mode => {
                let modes = GenerationMode::all();
                if forward {
                    self.mode_idx = (self.mode_idx + 1) % modes.len();
                } else {
                    self.mode_idx = (self.mode_idx + modes.len() - 1) % modes.len();
                }
                self.mode = modes[self.mode_idx];
            }
            FormFocus::Size => {
                let sizes = SizePreset::all();
                if forward {
                    self.size_idx = (self.size_idx + 1) % sizes.len();
                } else {
                    self.size_idx = (self.size_idx + sizes.len() - 1) % sizes.len();
                }
                self.size = sizes[self.size_idx];
            }
            FormFocus::Model => {
                if forward {
                    self.model_idx = (self.model_idx + 1) % self.available_models.len();
                } else {
                    self.model_idx = (self.model_idx + self.available_models.len() - 1)
                        % self.available_models.len();
                }
                self.model = self.available_models[self.model_idx].clone();
            }
            FormFocus::Denoise => {
                // Adjust denoise in 0.05 increments, clamped to 0.05-1.0
                if forward {
                    self.denoise = (self.denoise + 0.05).min(1.0);
                } else {
                    self.denoise = (self.denoise - 0.05).max(0.05);
                }
                // Round to avoid floating point errors
                self.denoise = (self.denoise * 100.0).round() / 100.0;
            }
            _ => {}
        }
    }
}

#[async_trait]
impl Screen for GeneratorScreen {
    fn draw(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // Reference image
                Constraint::Length(3),  // Mode
                Constraint::Length(3),  // Size
                Constraint::Length(3),  // Model
                Constraint::Length(3),  // Denoise
                Constraint::Length(5),  // Prompt
                Constraint::Length(3),  // Generate button
                Constraint::Min(0),     // Progress/Result
            ])
            .split(area);

        let focused_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
        let normal_style = Style::default().fg(Color::White);

        // Reference image
        let ref_text = self
            .reference_image
            .as_ref()
            .map(|p| p.as_str())
            .unwrap_or("No image selected (select from Browser)");
        let ref_widget = Paragraph::new(ref_text)
            .block(Block::default().borders(Borders::ALL).title("Reference Image"))
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(ref_widget, chunks[0]);

        // Mode selection
        let mode_style = if self.focus == FormFocus::Mode {
            focused_style
        } else {
            normal_style
        };
        let mode_text = format!("◄ {} ►", self.mode.name());
        let mode_widget = Paragraph::new(mode_text)
            .block(Block::default().borders(Borders::ALL).title("Mode"))
            .style(mode_style);
        f.render_widget(mode_widget, chunks[1]);

        // Size selection
        let size_style = if self.focus == FormFocus::Size {
            focused_style
        } else {
            normal_style
        };
        let size_text = format!("◄ {} ►", self.size.name());
        let size_widget = Paragraph::new(size_text)
            .block(Block::default().borders(Borders::ALL).title("Output Size"))
            .style(size_style);
        f.render_widget(size_widget, chunks[2]);

        // Model selection
        let model_style = if self.focus == FormFocus::Model {
            focused_style
        } else {
            normal_style
        };
        let model_display = if self.available_models.is_empty() {
            "No models loaded - press [r] to refresh".to_string()
        } else {
            format!("◄ {} ►", self.model)
        };
        let model_title = format!("Model ({}/{}) [r] refresh",
            if self.available_models.is_empty() { 0 } else { self.model_idx + 1 },
            self.available_models.len()
        );
        let model_widget = Paragraph::new(model_display)
            .block(Block::default().borders(Borders::ALL).title(model_title))
            .style(model_style);
        f.render_widget(model_widget, chunks[3]);

        // Denoise slider
        let denoise_style = if self.focus == FormFocus::Denoise {
            focused_style
        } else {
            normal_style
        };
        let denoise_pct = (self.denoise * 100.0) as u8;
        let denoise_label = match denoise_pct {
            0..=25 => "Subtle",
            26..=45 => "Light",
            46..=65 => "Moderate",
            66..=85 => "Strong",
            _ => "Heavy",
        };
        let denoise_text = format!("◄ {:.0}% ({}) ►", self.denoise * 100.0, denoise_label);
        let denoise_widget = Paragraph::new(denoise_text)
            .block(Block::default().borders(Borders::ALL).title("Denoise (lower = more original)"))
            .style(denoise_style);
        f.render_widget(denoise_widget, chunks[4]);

        // Prompt input
        let prompt_style = if self.focus == FormFocus::Prompt {
            focused_style
        } else {
            normal_style
        };
        let prompt_text = if self.prompt.is_empty() {
            "Enter a prompt to modify the image style..."
        } else {
            &self.prompt
        };
        let prompt_widget = Paragraph::new(prompt_text)
            .block(Block::default().borders(Borders::ALL).title("Prompt"))
            .style(if self.prompt.is_empty() && self.focus != FormFocus::Prompt {
                Style::default().fg(Color::DarkGray)
            } else {
                prompt_style
            })
            .wrap(Wrap { trim: true });
        f.render_widget(prompt_widget, chunks[5]);

        // Generate button
        let button_style = if self.focus == FormFocus::Generate {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Cyan)
        };
        let button_text = if self.generating {
            "⏳ Generating..."
        } else {
            "▶ Generate"
        };
        let button_widget = Paragraph::new(button_text)
            .block(Block::default().borders(Borders::ALL))
            .style(button_style)
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(button_widget, chunks[6]);

        // Progress/Result area
        let result_block = Block::default()
            .borders(Borders::ALL)
            .title("Generation Progress");

        if let Some(error) = &self.error {
            let error_widget = Paragraph::new(error.as_str())
                .block(result_block)
                .style(Style::default().fg(Color::Red))
                .wrap(Wrap { trim: true });
            f.render_widget(error_widget, chunks[7]);
        } else if let Some(progress) = &self.progress {
            let inner_area = result_block.inner(chunks[7]);
            f.render_widget(result_block, chunks[7]);

            let progress_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),  // Status line
                    Constraint::Length(1),  // Gauge
                    Constraint::Min(1),     // Logs
                ])
                .split(inner_area);

            // Status line
            let status = Paragraph::new(progress.status.as_str())
                .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
            f.render_widget(status, progress_layout[0]);

            // Progress gauge
            let gauge = Gauge::default()
                .gauge_style(Style::default().fg(Color::Cyan).bg(Color::DarkGray))
                .percent((progress.progress * 100.0) as u16)
                .label(format!(
                    "{}/{}",
                    progress.current_step, progress.total_steps
                ));
            f.render_widget(gauge, progress_layout[1]);

            // Logs - show last N lines that fit in the area
            let log_height = progress_layout[2].height as usize;
            let logs_to_show: Vec<&str> = progress.logs
                .iter()
                .rev()
                .take(log_height)
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();

            let log_items: Vec<ListItem> = logs_to_show
                .iter()
                .map(|log| {
                    let style = if log.starts_with("ERROR") {
                        Style::default().fg(Color::Red)
                    } else if log.starts_with("Step") {
                        Style::default().fg(Color::Yellow)
                    } else if log.starts_with("Executing") || log.starts_with("Execution") {
                        Style::default().fg(Color::Green)
                    } else if log.starts_with("Output") {
                        Style::default().fg(Color::Magenta)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };
                    ListItem::new(*log).style(style)
                })
                .collect();

            let logs_widget = List::new(log_items);
            f.render_widget(logs_widget, progress_layout[2]);
        } else if let Some(path) = &self.result_path {
            let result_widget = Paragraph::new(format!("✓ Generated: {}", path))
                .block(result_block)
                .style(Style::default().fg(Color::Green));
            f.render_widget(result_widget, chunks[7]);
        } else {
            let empty = Paragraph::new("Ready to generate")
                .block(result_block)
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(empty, chunks[7]);
        }
    }

    async fn handle_key(&mut self, key: KeyEvent) {
        if self.generating {
            // Only allow cancel during generation
            if key.code == KeyCode::Esc {
                let _ = self.comfyui_service.interrupt().await;
                self.generating = false;
            }
            return;
        }

        match key.code {
            KeyCode::Tab | KeyCode::Down => self.next_field(),
            KeyCode::BackTab | KeyCode::Up => self.prev_field(),
            KeyCode::Left => self.cycle_selection(false),
            KeyCode::Right => self.cycle_selection(true),
            KeyCode::Enter => {
                if self.focus == FormFocus::Generate {
                    self.start_generation().await;
                } else {
                    self.next_field();
                }
            }
            KeyCode::Char('r') if self.focus != FormFocus::Prompt => {
                // Refresh models from ComfyUI
                self.models_loaded = false;
                self.load_models().await;
            }
            KeyCode::Char(c) => {
                if self.focus == FormFocus::Prompt {
                    self.prompt.push(c);
                }
            }
            KeyCode::Backspace => {
                if self.focus == FormFocus::Prompt {
                    self.prompt.pop();
                }
            }
            _ => {}
        }
    }
}
