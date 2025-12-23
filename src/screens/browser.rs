//! JWST Image Browser screen.

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent};
use image::DynamicImage;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use ratatui_image::{picker::Picker, protocol::StatefulProtocol, Resize, StatefulImage};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

use super::Screen;
use crate::services::{DownloadProgress, EsaService, EsaImage, JwstApiService, WallustService};

/// Message for async preview loading
struct PreviewLoaded {
    image_id: String,
    image: DynamicImage,
}

/// Download state for progress display
#[derive(Clone)]
struct DownloadState {
    downloaded: u64,
    total: Option<u64>,
}

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

    // Image preview state
    image_picker: Option<Picker>,
    preview_source: Option<DynamicImage>,  // Source image for preview
    preview_protocol: Option<StatefulProtocol>,  // Rendered protocol (recreated on resize)
    preview_image_id: Option<String>,
    last_preview_area: Option<Rect>,  // Track area to detect resize

    // Async preview loading
    preview_rx: mpsc::Receiver<PreviewLoaded>,
    preview_tx: mpsc::Sender<PreviewLoaded>,
    loading_preview_id: Option<String>,

    // Download progress
    download_progress_rx: mpsc::Receiver<DownloadProgress>,
    download_progress_tx: mpsc::Sender<DownloadProgress>,
    download_state: Option<DownloadState>,
}

impl BrowserScreen {
    pub fn new(
        esa_service: Arc<EsaService>,
        api_service: Arc<JwstApiService>,
        wallust_service: Arc<WallustService>,
    ) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));

        // Try to create an image picker for the current terminal
        let image_picker = Picker::from_query_stdio().ok();

        // Channel for async preview loading (buffer of 1 since we only care about latest)
        let (preview_tx, preview_rx) = mpsc::channel(1);

        // Channel for download progress
        let (download_progress_tx, download_progress_rx) = mpsc::channel(32);

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
            image_picker,
            preview_source: None,
            preview_protocol: None,
            preview_image_id: None,
            last_preview_area: None,
            preview_rx,
            preview_tx,
            loading_preview_id: None,
            download_progress_rx,
            download_progress_tx,
            download_state: None,
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

    /// Check for completed preview loads and update state.
    fn poll_preview_loaded(&mut self) {
        // Try to receive any completed preview loads
        while let Ok(loaded) = self.preview_rx.try_recv() {
            // Only update display if this is still the image we want
            if self.loading_preview_id.as_ref() == Some(&loaded.image_id) {
                self.preview_source = Some(loaded.image);
                self.preview_protocol = None;  // Will be created in draw() with correct size
                self.preview_image_id = Some(loaded.image_id);
                self.last_preview_area = None;  // Force protocol recreation
                self.loading_preview_id = None;
            }
        }
    }

    /// Start async preview loading for the currently selected item.
    /// Only loads previews for already-downloaded images.
    fn start_preview_load(&mut self) {
        if self.image_picker.is_none() {
            return;
        }

        let Some(image) = self.selected_image().cloned() else {
            self.preview_source = None;
            self.preview_protocol = None;
            self.preview_image_id = None;
            self.loading_preview_id = None;
            return;
        };

        // Skip if already displaying this image
        if self.preview_image_id.as_ref() == Some(&image.id) {
            return;
        }

        // Skip if already loading this image
        if self.loading_preview_id.as_ref() == Some(&image.id) {
            return;
        }

        // Only load preview for already-downloaded images
        let Some(source_path) = self.esa_service.get_downloaded_path(&image) else {
            // Not downloaded - clear preview
            self.preview_source = None;
            self.preview_protocol = None;
            self.preview_image_id = None;
            self.loading_preview_id = None;
            return;
        };

        // Mark as loading
        self.loading_preview_id = Some(image.id.clone());

        // Spawn background task to load the image
        let tx = self.preview_tx.clone();
        let image_id = image.id.clone();

        tokio::spawn(async move {
            // Load image in blocking thread pool to not block async runtime
            let result = tokio::task::spawn_blocking(move || {
                image::open(&source_path).ok()
            }).await;

            if let Ok(Some(img)) = result {
                // Send back to main thread (ignore error if receiver dropped)
                let _ = tx.send(PreviewLoaded { image_id, image: img }).await;
            }
        });
    }

    /// Download the currently selected image (without applying as wallpaper).
    fn start_download(&mut self) {
        let Some(image) = self.selected_image().cloned() else {
            return;
        };

        // Already downloading
        if self.download_state.is_some() {
            return;
        }

        self.download_state = Some(DownloadState {
            downloaded: 0,
            total: None,
        });
        self.error = None;

        let esa_service = self.esa_service.clone();
        let progress_tx = self.download_progress_tx.clone();

        tokio::spawn(async move {
            let result = esa_service
                .download_image_with_progress(&image, "wallpaper_uhd", progress_tx)
                .await;

            // Signal completion by sending a "done" message (downloaded == total)
            if result.is_err() {
                // Error case - we'll detect by download_state being cleared
            }
        });
    }

    /// Poll for download progress updates.
    fn poll_download_progress(&mut self) {
        while let Ok(progress) = self.download_progress_rx.try_recv() {
            self.download_state = Some(DownloadState {
                downloaded: progress.downloaded,
                total: progress.total,
            });

            // Check if download is complete
            if let Some(total) = progress.total {
                if progress.downloaded >= total {
                    self.download_state = None;
                    // Trigger preview reload for newly downloaded image
                    self.start_preview_load();
                }
            }
        }
    }

    /// Get the best reference image path for the generator.
    /// Priority: currently selected image's download path > last_downloaded
    pub fn get_reference_image_path(&self) -> Option<PathBuf> {
        // First, check if the currently selected image is downloaded
        if let Some(image) = self.selected_image() {
            if let Some(path) = self.esa_service.get_downloaded_path(image) {
                return Some(path);
            }
        }

        // Fallback to last explicitly downloaded image
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
        // Poll for updates
        self.poll_download_progress();
        // Poll for completed preview loads
        self.poll_preview_loaded();

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

        // Right side: Split into preview and details
        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(60), // Image preview
                Constraint::Percentage(40), // Details
            ])
            .split(chunks[1]);

        // Image preview area
        let preview_block = Block::default()
            .borders(Borders::ALL)
            .title("Preview");

        let inner_area = preview_block.inner(right_chunks[0]);

        // Recreate protocol if we have a source image and area changed
        if self.preview_source.is_some() {
            let area_changed = self.last_preview_area != Some(inner_area);
            if area_changed || self.preview_protocol.is_none() {
                if let (Some(picker), Some(source)) = (&self.image_picker, &self.preview_source) {
                    // Create protocol sized for the preview area
                    let protocol = picker.new_resize_protocol(source.clone());
                    self.preview_protocol = Some(protocol);
                    self.last_preview_area = Some(inner_area);
                }
            }
        }

        if let Some(ref state) = self.download_state {
            // Show download progress
            let progress_text = if let Some(total) = state.total {
                let percent = (state.downloaded as f64 / total as f64 * 100.0) as u8;
                let downloaded_mb = state.downloaded as f64 / 1_000_000.0;
                let total_mb = total as f64 / 1_000_000.0;
                format!(
                    "Downloading... {}%\n{:.1} / {:.1} MB",
                    percent, downloaded_mb, total_mb
                )
            } else {
                let downloaded_mb = state.downloaded as f64 / 1_000_000.0;
                format!("Downloading... {:.1} MB", downloaded_mb)
            };
            let downloading = Paragraph::new(progress_text)
                .block(preview_block)
                .style(Style::default().fg(Color::Cyan));
            f.render_widget(downloading, right_chunks[0]);
        } else if let Some(ref mut protocol) = self.preview_protocol {
            f.render_widget(preview_block, right_chunks[0]);

            // Render the image scaled to fill the area
            let image_widget = StatefulImage::new().resize(Resize::Scale(None));
            f.render_stateful_widget(image_widget, inner_area, protocol);
        } else if self.loading_preview_id.is_some() {
            let loading = Paragraph::new("Loading preview...")
                .block(preview_block)
                .style(Style::default().fg(Color::Yellow));
            f.render_widget(loading, right_chunks[0]);
        } else if self.image_picker.is_none() {
            let no_support = Paragraph::new("Terminal doesn't support image display")
                .block(preview_block)
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(no_support, right_chunks[0]);
        } else {
            let no_preview = Paragraph::new("Download image to see preview")
                .block(preview_block)
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(no_preview, right_chunks[0]);
        }

        // Image details area
        let detail_block = Block::default()
            .borders(Borders::ALL)
            .title("Details");

        if self.loading {
            let loading = Paragraph::new("Loading...")
                .block(detail_block)
                .style(Style::default().fg(Color::Yellow));
            f.render_widget(loading, right_chunks[1]);
        } else if let Some(error) = &self.error {
            let error_widget = Paragraph::new(error.as_str())
                .block(detail_block)
                .style(Style::default().fg(Color::Red))
                .wrap(Wrap { trim: true });
            f.render_widget(error_widget, right_chunks[1]);
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
                Line::from(vec![
                    Span::styled("Title: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&image.title, Style::default().fg(Color::White)),
                ]),
                Line::from(vec![
                    Span::styled("Published: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(date, Style::default().fg(Color::White)),
                ]),
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
            ];

            let detail = Paragraph::new(details)
                .block(detail_block)
                .wrap(Wrap { trim: true });
            f.render_widget(detail, right_chunks[1]);
        } else {
            let empty = Paragraph::new("No image selected")
                .block(detail_block)
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(empty, right_chunks[1]);
        }
    }

    async fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.previous();
                self.start_preview_load();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.next();
                self.start_preview_load();
            }
            KeyCode::Enter => {
                self.start_download();
            }
            KeyCode::Char('w') => {
                // Apply as wallpaper (runs in background)
                self.apply_as_wallpaper();
            }
            KeyCode::Char('r') => {
                let _ = self.load_images(true).await;
                // Load preview for first image after refresh
                self.start_preview_load();
            }
            KeyCode::Char('s') => {
                // Toggle source
                self.source = match self.source {
                    ImageSource::Esa => ImageSource::JwstApi,
                    ImageSource::JwstApi => ImageSource::Esa,
                };
                let _ = self.load_images(false).await;
                self.start_preview_load();
            }
            KeyCode::Char(' ') => {
                self.show_detail = !self.show_detail;
            }
            KeyCode::Char('p') => {
                // Manual preview load
                self.start_preview_load();
            }
            _ => {}
        }
    }
}
