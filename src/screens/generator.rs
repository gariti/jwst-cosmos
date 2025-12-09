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
use std::sync::Arc;

use super::Screen;
use crate::config::Config;
use crate::services::{ComfyUiService, GenerationProgress, OllamaService, WallustService};
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
    Prompt,
    Model,
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

    // Reference image
    reference_image: Option<String>,

    // Generation state
    generating: bool,
    progress: Option<GenerationProgress>,
    result_path: Option<String>,
    error: Option<String>,
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
            model: "sdxl".to_string(),
            available_models: vec!["sdxl".to_string(), "flux".to_string()],
            model_idx: 0,
            reference_image: None,
            generating: false,
            progress: None,
            result_path: None,
            error: None,
        }
    }

    /// Set the reference image from browser.
    pub fn set_reference_image(&mut self, path: String) {
        self.reference_image = Some(path);
    }

    /// Load available models from ComfyUI.
    pub async fn load_models(&mut self) {
        if let Ok(models) = self.comfyui_service.get_checkpoints().await {
            if !models.is_empty() {
                self.available_models = models;
                self.model_idx = 0;
                self.model = self.available_models[0].clone();
            }
        }
    }

    /// Start generation.
    async fn start_generation(&mut self) {
        if self.reference_image.is_none() {
            self.error = Some("No reference image selected".to_string());
            return;
        }

        self.generating = true;
        self.error = None;
        self.progress = Some(GenerationProgress {
            status: "Starting...".to_string(),
            progress: 0.0,
            current_step: 0,
            total_steps: 0,
            node_id: None,
        });

        // Build generation parameters
        let (width, height) = self.size.dimensions();
        let mut params = HashMap::new();
        params.insert("width".to_string(), width.to_string());
        params.insert("height".to_string(), height.to_string());
        params.insert("prompt".to_string(), self.prompt.clone());
        params.insert("model".to_string(), self.model.clone());
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

        // TODO: Spawn generation task and handle progress
        // This would involve the actual ComfyUI generation which is async

        self.generating = false;
    }

    /// Navigate to next form field.
    fn next_field(&mut self) {
        self.focus = match self.focus {
            FormFocus::Mode => FormFocus::Size,
            FormFocus::Size => FormFocus::Model,
            FormFocus::Model => FormFocus::Prompt,
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
            FormFocus::Prompt => FormFocus::Model,
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
        let model_text = format!("◄ {} ►", self.model);
        let model_widget = Paragraph::new(model_text)
            .block(Block::default().borders(Borders::ALL).title("Model"))
            .style(model_style);
        f.render_widget(model_widget, chunks[3]);

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
        f.render_widget(prompt_widget, chunks[4]);

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
        f.render_widget(button_widget, chunks[5]);

        // Progress/Result area
        let result_block = Block::default()
            .borders(Borders::ALL)
            .title("Generation Progress");

        if let Some(error) = &self.error {
            let error_widget = Paragraph::new(error.as_str())
                .block(result_block)
                .style(Style::default().fg(Color::Red))
                .wrap(Wrap { trim: true });
            f.render_widget(error_widget, chunks[6]);
        } else if let Some(progress) = &self.progress {
            let progress_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Length(3)])
                .split(chunks[6]);

            let status = Paragraph::new(progress.status.as_str())
                .block(result_block);
            f.render_widget(status, progress_layout[0]);

            let gauge = Gauge::default()
                .gauge_style(Style::default().fg(Color::Cyan).bg(Color::DarkGray))
                .percent((progress.progress * 100.0) as u16)
                .label(format!(
                    "{}/{}",
                    progress.current_step, progress.total_steps
                ));
            f.render_widget(gauge, progress_layout[1]);
        } else if let Some(path) = &self.result_path {
            let result_widget = Paragraph::new(format!("✓ Generated: {}", path))
                .block(result_block)
                .style(Style::default().fg(Color::Green));
            f.render_widget(result_widget, chunks[6]);
        } else {
            let empty = Paragraph::new("Ready to generate")
                .block(result_block)
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(empty, chunks[6]);
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
