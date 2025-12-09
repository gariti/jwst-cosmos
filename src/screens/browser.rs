//! JWST Image Browser screen.

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use std::path::PathBuf;
use std::sync::Arc;

use super::Screen;
use crate::services::{EsaService, EsaImage, JwstApiService, WallustService};

/// Image source selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageSource {
    Esa,
    JwstApi,
}

/// Browser screen state.
pub struct BrowserScreen {
    esa_service: Arc<EsaService>,
    api_service: Arc<JwstApiService>,
    wallust_service: Arc<WallustService>,

    // State
    source: ImageSource,
    esa_images: Vec<EsaImage>,
    list_state: ListState,
    loading: bool,
    error: Option<String>,

    // Selected image for detail view
    show_detail: bool,

    // Track last downloaded image for wallpaper application
    last_downloaded: Option<PathBuf>,
}

impl BrowserScreen {
    pub fn new(
        esa_service: Arc<EsaService>,
        api_service: Arc<JwstApiService>,
        wallust_service: Arc<WallustService>,
    ) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));

        Self {
            esa_service,
            api_service,
            wallust_service,
            source: ImageSource::Esa,
            esa_images: Vec::new(),
            list_state,
            loading: false,
            error: None,
            show_detail: false,
            last_downloaded: None,
        }
    }

    /// Load images from the current source.
    pub async fn load_images(&mut self, force_refresh: bool) -> anyhow::Result<()> {
        self.loading = true;
        self.error = None;

        match self.source {
            ImageSource::Esa => {
                match self.esa_service.get_images(force_refresh).await {
                    Ok(images) => {
                        self.esa_images = images;
                        if !self.esa_images.is_empty() {
                            self.list_state.select(Some(0));
                        }
                    }
                    Err(e) => {
                        self.error = Some(format!("Failed to load images: {}", e));
                    }
                }
            }
            ImageSource::JwstApi => {
                // TODO: Implement JWST API loading
            }
        }

        self.loading = false;
        Ok(())
    }

    /// Get the currently selected image.
    fn selected_image(&self) -> Option<&EsaImage> {
        self.list_state.selected().and_then(|i| self.esa_images.get(i))
    }

    /// Download the currently selected image (without applying as wallpaper).
    async fn download_selected(&mut self) -> anyhow::Result<()> {
        if let Some(image) = self.selected_image().cloned() {
            self.loading = true;
            self.error = None;
            match self.esa_service.download_image(&image, "wallpaper_uhd").await {
                Ok(path) => {
                    self.last_downloaded = Some(path);
                    // Don't auto-apply - user can press 'w' to apply
                }
                Err(e) => {
                    self.error = Some(format!("Download failed: {}", e));
                }
            }
            self.loading = false;
        }
        Ok(())
    }

    /// Get the best reference image path for the generator.
    /// Priority: currently selected image's download path > last_downloaded
    pub fn get_reference_image_path(&self) -> Option<PathBuf> {
        // First, check if the currently selected image is downloaded
        if let Some(image) = self.selected_image() {
            eprintln!("[DEBUG] Selected image ID: {}", image.id);
            if let Some(path) = self.esa_service.get_downloaded_path(image) {
                eprintln!("[DEBUG] Found downloaded path: {:?}", path);
                return Some(path);
            } else {
                eprintln!("[DEBUG] No downloaded path found for selected image");
            }
        } else {
            eprintln!("[DEBUG] No image selected");
        }

        // Fallback to last explicitly downloaded image
        eprintln!("[DEBUG] Falling back to last_downloaded: {:?}", self.last_downloaded);
        self.last_downloaded.clone()
    }

    /// Apply the selected (or last downloaded) image as wallpaper.
    fn apply_as_wallpaper(&mut self) {
        // First check if we have a downloaded path for the selected image
        if let Some(image) = self.selected_image() {
            if let Some(path) = self.esa_service.get_downloaded_path(image) {
                if let Err(e) = self.wallust_service.apply_wallpaper(&path) {
                    self.error = Some(format!("Failed to apply wallpaper: {}", e));
                } else {
                    self.error = None;
                }
                return;
            }
        }

        // Fallback to last downloaded
        if let Some(path) = &self.last_downloaded {
            if let Err(e) = self.wallust_service.apply_wallpaper(path) {
                self.error = Some(format!("Failed to apply wallpaper: {}", e));
            } else {
                self.error = None;
            }
        } else {
            self.error = Some("No image downloaded yet - press Enter to download first".to_string());
        }
    }

    /// Move selection up.
    fn previous(&mut self) {
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.esa_images.len().saturating_sub(1)
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
        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= self.esa_images.len().saturating_sub(1) {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }
}

#[async_trait]
impl Screen for BrowserScreen {
    fn draw(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50),
                Constraint::Percentage(50),
            ])
            .split(area);

        // Left side: Image list
        let items: Vec<ListItem> = self
            .esa_images
            .iter()
            .map(|img| {
                let downloaded = self.esa_service.is_downloaded(img);
                let marker = if downloaded { "✓ " } else { "  " };

                let date = img
                    .pub_date
                    .map(|d| d.format("%Y-%m-%d").to_string())
                    .unwrap_or_else(|| "Unknown".to_string());

                let content = format!("{}{} - {}", marker, img.id, date);

                ListItem::new(Line::from(vec![
                    Span::styled(marker, Style::default().fg(Color::Green)),
                    Span::styled(img.id.clone(), Style::default().fg(Color::Cyan)),
                    Span::raw(" - "),
                    Span::styled(date, Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect();

        let source_name = match self.source {
            ImageSource::Esa => "ESA/Webb Gallery",
            ImageSource::JwstApi => "JWST API",
        };

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!("Images ({})", source_name))
                    .title_bottom(Line::from(vec![
                        Span::styled("[↑/↓]", Style::default().fg(Color::DarkGray)),
                        Span::raw(" Nav "),
                        Span::styled("[Enter]", Style::default().fg(Color::DarkGray)),
                        Span::raw(" DL "),
                        Span::styled("[w]", Style::default().fg(Color::DarkGray)),
                        Span::raw(" Wallpaper "),
                        Span::styled("[r]", Style::default().fg(Color::DarkGray)),
                        Span::raw(" Refresh"),
                    ])),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("► ");

        f.render_stateful_widget(list, chunks[0], &mut self.list_state);

        // Right side: Image details
        let detail_block = Block::default()
            .borders(Borders::ALL)
            .title("Image Details");

        if self.loading {
            let loading = Paragraph::new("Loading...")
                .block(detail_block)
                .style(Style::default().fg(Color::Yellow));
            f.render_widget(loading, chunks[1]);
        } else if let Some(error) = &self.error {
            let error_widget = Paragraph::new(error.as_str())
                .block(detail_block)
                .style(Style::default().fg(Color::Red))
                .wrap(Wrap { trim: true });
            f.render_widget(error_widget, chunks[1]);
        } else if let Some(image) = self.selected_image() {
            let date = image
                .pub_date
                .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "Unknown".to_string());

            let downloaded = if self.esa_service.is_downloaded(image) {
                "Yes"
            } else {
                "No"
            };

            let details = vec![
                Line::from(vec![
                    Span::styled("ID: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&image.id, Style::default().fg(Color::Cyan)),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Title: ", Style::default().fg(Color::DarkGray)),
                ]),
                Line::from(Span::styled(
                    &image.title,
                    Style::default().fg(Color::White),
                )),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Published: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(date, Style::default().fg(Color::White)),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Downloaded: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        downloaded,
                        if downloaded == "Yes" {
                            Style::default().fg(Color::Green)
                        } else {
                            Style::default().fg(Color::Yellow)
                        },
                    ),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Gallery URL: ", Style::default().fg(Color::DarkGray)),
                ]),
                Line::from(Span::styled(
                    image.gallery_url(),
                    Style::default().fg(Color::Blue),
                )),
            ];

            let detail = Paragraph::new(details)
                .block(detail_block)
                .wrap(Wrap { trim: true });
            f.render_widget(detail, chunks[1]);
        } else {
            let empty = Paragraph::new("No image selected")
                .block(detail_block)
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(empty, chunks[1]);
        }
    }

    async fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.previous(),
            KeyCode::Down | KeyCode::Char('j') => self.next(),
            KeyCode::Enter => {
                let _ = self.download_selected().await;
            }
            KeyCode::Char('w') => {
                // Apply as wallpaper (runs in background)
                self.apply_as_wallpaper();
            }
            KeyCode::Char('r') => {
                let _ = self.load_images(true).await;
            }
            KeyCode::Char('s') => {
                // Toggle source
                self.source = match self.source {
                    ImageSource::Esa => ImageSource::JwstApi,
                    ImageSource::JwstApi => ImageSource::Esa,
                };
                let _ = self.load_images(false).await;
            }
            KeyCode::Char(' ') => {
                self.show_detail = !self.show_detail;
            }
            _ => {}
        }
    }
}
