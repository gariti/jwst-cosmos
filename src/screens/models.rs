//! Model management screen.

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, ListState, Paragraph, Tabs, Wrap},
    Frame,
};
use std::sync::Arc;

use super::Screen;
use crate::services::{ComfyUiService, OllamaService, OllamaModel, PullProgress};

/// Tab selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelTab {
    Ollama,
    ComfyUI,
}

/// Models screen state.
pub struct ModelsScreen {
    ollama_service: Arc<OllamaService>,
    comfyui_service: Arc<ComfyUiService>,

    // UI state
    current_tab: ModelTab,
    ollama_models: Vec<OllamaModel>,
    comfyui_models: Vec<String>,
    list_state: ListState,
    loading: bool,
    error: Option<String>,

    // Pull state
    pulling: bool,
    pull_model: String,
    pull_progress: Option<PullProgress>,

    // Input mode for pulling new models
    input_mode: bool,
    input_buffer: String,
}

impl ModelsScreen {
    pub fn new(
        ollama_service: Arc<OllamaService>,
        comfyui_service: Arc<ComfyUiService>,
    ) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));

        Self {
            ollama_service,
            comfyui_service,
            current_tab: ModelTab::Ollama,
            ollama_models: Vec::new(),
            comfyui_models: Vec::new(),
            list_state,
            loading: false,
            error: None,
            pulling: false,
            pull_model: String::new(),
            pull_progress: None,
            input_mode: false,
            input_buffer: String::new(),
        }
    }

    /// Load models from the current tab's service.
    pub async fn load_models(&mut self) {
        self.loading = true;
        self.error = None;

        match self.current_tab {
            ModelTab::Ollama => {
                match self.ollama_service.list_models().await {
                    Ok(models) => {
                        self.ollama_models = models;
                        if !self.ollama_models.is_empty() {
                            self.list_state.select(Some(0));
                        }
                    }
                    Err(e) => {
                        self.error = Some(format!("Failed to load Ollama models: {}", e));
                    }
                }
            }
            ModelTab::ComfyUI => {
                match self.comfyui_service.get_checkpoints().await {
                    Ok(models) => {
                        self.comfyui_models = models;
                        if !self.comfyui_models.is_empty() {
                            self.list_state.select(Some(0));
                        }
                    }
                    Err(e) => {
                        self.error = Some(format!("Failed to load ComfyUI models: {}", e));
                    }
                }
            }
        }

        self.loading = false;
    }

    /// Pull a new Ollama model.
    async fn pull_model(&mut self, model_name: &str) {
        self.pulling = true;
        self.pull_model = model_name.to_string();
        self.error = None;

        match self.ollama_service.pull_model(model_name).await {
            Ok(mut rx) => {
                // In a real implementation, we'd spawn a task to handle this
                // For now, just indicate it started
                self.pull_progress = Some(PullProgress {
                    status: "Starting download...".to_string(),
                    digest: None,
                    total: None,
                    completed: None,
                });
            }
            Err(e) => {
                self.error = Some(format!("Failed to pull model: {}", e));
                self.pulling = false;
            }
        }
    }

    /// Delete the selected Ollama model.
    async fn delete_selected(&mut self) {
        if self.current_tab != ModelTab::Ollama {
            return;
        }

        if let Some(idx) = self.list_state.selected() {
            if let Some(model) = self.ollama_models.get(idx) {
                let name = model.name.clone();
                match self.ollama_service.delete_model(&name).await {
                    Ok(_) => {
                        self.load_models().await;
                    }
                    Err(e) => {
                        self.error = Some(format!("Failed to delete model: {}", e));
                    }
                }
            }
        }
    }

    /// Get the count of models in current tab.
    fn model_count(&self) -> usize {
        match self.current_tab {
            ModelTab::Ollama => self.ollama_models.len(),
            ModelTab::ComfyUI => self.comfyui_models.len(),
        }
    }

    /// Move selection up.
    fn previous(&mut self) {
        let count = self.model_count();
        if count == 0 {
            return;
        }

        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    count - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    /// Move selection down.
    fn next(&mut self) {
        let count = self.model_count();
        if count == 0 {
            return;
        }

        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= count - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    /// Switch tabs.
    fn switch_tab(&mut self) {
        self.current_tab = match self.current_tab {
            ModelTab::Ollama => ModelTab::ComfyUI,
            ModelTab::ComfyUI => ModelTab::Ollama,
        };
        self.list_state.select(Some(0));
    }
}

#[async_trait]
impl Screen for ModelsScreen {
    fn draw(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // Tabs
                Constraint::Min(0),     // Model list
                Constraint::Length(3),  // Input/Status
            ])
            .split(area);

        // Tab bar
        let tab_titles: Vec<Line> = vec!["Ollama", "ComfyUI"]
            .into_iter()
            .map(Line::from)
            .collect();
        let selected_tab = match self.current_tab {
            ModelTab::Ollama => 0,
            ModelTab::ComfyUI => 1,
        };
        let tabs = Tabs::new(tab_titles)
            .block(Block::default().borders(Borders::ALL).title("Model Sources"))
            .select(selected_tab)
            .style(Style::default().fg(Color::White))
            .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
        f.render_widget(tabs, chunks[0]);

        // Model list
        let list_block = Block::default()
            .borders(Borders::ALL)
            .title("Models")
            .title_bottom(Line::from(vec![
                Span::styled("[Tab]", Style::default().fg(Color::DarkGray)),
                Span::raw(" Switch "),
                Span::styled("[p]", Style::default().fg(Color::DarkGray)),
                Span::raw(" Pull "),
                Span::styled("[d]", Style::default().fg(Color::DarkGray)),
                Span::raw(" Delete "),
                Span::styled("[r]", Style::default().fg(Color::DarkGray)),
                Span::raw(" Refresh"),
            ]));

        if self.loading {
            let loading = Paragraph::new("Loading models...")
                .block(list_block)
                .style(Style::default().fg(Color::Yellow));
            f.render_widget(loading, chunks[1]);
        } else if let Some(error) = &self.error {
            let error_widget = Paragraph::new(error.as_str())
                .block(list_block)
                .style(Style::default().fg(Color::Red))
                .wrap(Wrap { trim: true });
            f.render_widget(error_widget, chunks[1]);
        } else {
            let items: Vec<ListItem> = match self.current_tab {
                ModelTab::Ollama => {
                    self.ollama_models
                        .iter()
                        .map(|model| {
                            let vision_marker = if model.is_vision_model() {
                                "👁 "
                            } else {
                                "  "
                            };

                            let content = Line::from(vec![
                                Span::raw(vision_marker),
                                Span::styled(&model.name, Style::default().fg(Color::Cyan)),
                                Span::raw(" - "),
                                Span::styled(
                                    model.size_str(),
                                    Style::default().fg(Color::DarkGray),
                                ),
                            ]);

                            ListItem::new(content)
                        })
                        .collect()
                }
                ModelTab::ComfyUI => {
                    self.comfyui_models
                        .iter()
                        .map(|model| {
                            ListItem::new(Line::from(Span::styled(
                                model,
                                Style::default().fg(Color::Cyan),
                            )))
                        })
                        .collect()
                }
            };

            let list = List::new(items)
                .block(list_block)
                .highlight_style(
                    Style::default()
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("► ");

            f.render_stateful_widget(list, chunks[1], &mut self.list_state);
        }

        // Input/Status bar
        if self.input_mode {
            let input = Paragraph::new(format!("Pull model: {}_", self.input_buffer))
                .block(Block::default().borders(Borders::ALL).title("Enter model name"))
                .style(Style::default().fg(Color::Yellow));
            f.render_widget(input, chunks[2]);
        } else if self.pulling {
            let progress_text = if let Some(progress) = &self.pull_progress {
                let pct = match (progress.completed, progress.total) {
                    (Some(c), Some(t)) if t > 0 => format!(" ({:.1}%)", (c as f64 / t as f64) * 100.0),
                    _ => String::new(),
                };
                format!("{}: {}{}", self.pull_model, progress.status, pct)
            } else {
                format!("Pulling {}...", self.pull_model)
            };

            let progress_widget = Paragraph::new(progress_text)
                .block(Block::default().borders(Borders::ALL).title("Download Progress"))
                .style(Style::default().fg(Color::Cyan));
            f.render_widget(progress_widget, chunks[2]);
        } else {
            let status = match self.current_tab {
                ModelTab::Ollama => {
                    let vision_count = self.ollama_models.iter().filter(|m| m.is_vision_model()).count();
                    format!(
                        "{} models ({} vision)",
                        self.ollama_models.len(),
                        vision_count
                    )
                }
                ModelTab::ComfyUI => {
                    format!("{} checkpoints", self.comfyui_models.len())
                }
            };

            let status_widget = Paragraph::new(status)
                .block(Block::default().borders(Borders::ALL))
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(status_widget, chunks[2]);
        }
    }

    async fn handle_key(&mut self, key: KeyEvent) {
        if self.input_mode {
            match key.code {
                KeyCode::Enter => {
                    if !self.input_buffer.is_empty() {
                        let model_name = self.input_buffer.clone();
                        self.input_buffer.clear();
                        self.input_mode = false;
                        self.pull_model(&model_name).await;
                    }
                }
                KeyCode::Esc => {
                    self.input_buffer.clear();
                    self.input_mode = false;
                }
                KeyCode::Char(c) => {
                    self.input_buffer.push(c);
                }
                KeyCode::Backspace => {
                    self.input_buffer.pop();
                }
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Tab => self.switch_tab(),
            KeyCode::Up | KeyCode::Char('k') => self.previous(),
            KeyCode::Down | KeyCode::Char('j') => self.next(),
            KeyCode::Char('r') => {
                self.load_models().await;
            }
            KeyCode::Char('p') => {
                if self.current_tab == ModelTab::Ollama && !self.pulling {
                    self.input_mode = true;
                }
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                self.delete_selected().await;
            }
            _ => {}
        }
    }
}
