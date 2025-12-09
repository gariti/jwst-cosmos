//! Main application state and event loop.

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};
use std::io;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::config::Config;
use crate::screens::{Screen, BrowserScreen, GeneratorScreen, ModelsScreen};
use crate::services::{
    EsaService, JwstApiService, TunnelManager, OllamaService, ComfyUiService, WallustService,
};

/// Application state.
pub struct App {
    config: Arc<Config>,
    current_screen: AppScreen,
    should_quit: bool,

    // Services
    esa_service: Arc<EsaService>,
    api_service: Arc<JwstApiService>,
    tunnel_manager: Arc<tokio::sync::Mutex<TunnelManager>>,
    ollama_service: Arc<OllamaService>,
    comfyui_service: Arc<ComfyUiService>,
    wallust_service: Arc<WallustService>,

    // Screens
    browser_screen: BrowserScreen,
    generator_screen: GeneratorScreen,
    models_screen: ModelsScreen,

    // Status bar info
    status_message: String,
    tunnel_status: TunnelStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppScreen {
    Browser,
    Generator,
    Models,
}

#[derive(Debug, Clone, Default)]
pub struct TunnelStatus {
    pub ollama: bool,
    pub comfyui: bool,
}

impl App {
    /// Create a new application instance.
    pub fn new(config: Config) -> Result<Self> {
        let config = Arc::new(config);

        // Initialize services
        let esa_service = Arc::new(EsaService::new(config.clone()));
        let api_service = Arc::new(JwstApiService::new(config.clone()));
        let tunnel_manager = Arc::new(tokio::sync::Mutex::new(TunnelManager::new(config.clone())));
        let ollama_service = Arc::new(OllamaService::new());
        let comfyui_service = Arc::new(ComfyUiService::new());
        let wallust_service = Arc::new(WallustService::new((*config).clone()));

        // Initialize screens
        let browser_screen = BrowserScreen::new(
            esa_service.clone(),
            api_service.clone(),
            wallust_service.clone(),
        );
        let generator_screen = GeneratorScreen::new(
            comfyui_service.clone(),
            ollama_service.clone(),
            wallust_service.clone(),
            config.clone(),
        );
        let models_screen = ModelsScreen::new(
            ollama_service.clone(),
            comfyui_service.clone(),
        );

        Ok(Self {
            config,
            current_screen: AppScreen::Browser,
            should_quit: false,
            esa_service,
            api_service,
            tunnel_manager,
            ollama_service,
            comfyui_service,
            wallust_service,
            browser_screen,
            generator_screen,
            models_screen,
            status_message: "Ready".to_string(),
            tunnel_status: TunnelStatus::default(),
        })
    }

    /// Run the application.
    pub async fn run(&mut self) -> Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Load initial data
        self.load_initial_data().await;

        // Main event loop
        let result = self.event_loop(&mut terminal).await;

        // Restore terminal
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        result
    }

    /// Load initial data for all screens.
    async fn load_initial_data(&mut self) {
        self.status_message = "Loading images...".to_string();

        // Load ESA images
        if let Err(e) = self.browser_screen.load_images(false).await {
            self.status_message = format!("Failed to load images: {}", e);
        } else {
            self.status_message = "Ready".to_string();
        }
    }

    /// Main event loop.
    async fn event_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        loop {
            // Draw UI
            terminal.draw(|f| self.draw(f))?;

            // Poll for events with timeout
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    // Global key handlers
                    match (key.modifiers, key.code) {
                        (KeyModifiers::CONTROL, KeyCode::Char('c')) |
                        (KeyModifiers::CONTROL, KeyCode::Char('q')) |
                        (_, KeyCode::Char('q')) if self.current_screen == AppScreen::Browser => {
                            self.should_quit = true;
                        }
                        (_, KeyCode::Tab) => {
                            // Cycle to next tab
                            self.current_screen = match self.current_screen {
                                AppScreen::Browser => AppScreen::Generator,
                                AppScreen::Generator => AppScreen::Models,
                                AppScreen::Models => AppScreen::Browser,
                            };

                            // Sync selected image to generator when switching TO generator
                            if self.current_screen == AppScreen::Generator {
                                eprintln!("[DEBUG] Switching to Generator, attempting sync...");
                                if let Some(path) = self.browser_screen.get_reference_image_path() {
                                    eprintln!("[DEBUG] Setting reference image to: {:?}", path);
                                    self.generator_screen.set_reference_image(path.to_string_lossy().to_string());
                                } else {
                                    eprintln!("[DEBUG] No reference image path available");
                                }
                            }
                        }
                        (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                            // Cycle to previous tab (Shift+Tab)
                            self.current_screen = match self.current_screen {
                                AppScreen::Browser => AppScreen::Models,
                                AppScreen::Generator => AppScreen::Browser,
                                AppScreen::Models => AppScreen::Generator,
                            };

                            // Sync selected image to generator when switching TO generator
                            if self.current_screen == AppScreen::Generator {
                                if let Some(path) = self.browser_screen.get_reference_image_path() {
                                    self.generator_screen.set_reference_image(path.to_string_lossy().to_string());
                                }
                            }
                        }
                        (_, KeyCode::Char('t')) => {
                            // Toggle tunnel
                            self.toggle_tunnels().await;
                        }
                        _ => {
                            // Delegate to current screen
                            match self.current_screen {
                                AppScreen::Browser => {
                                    self.browser_screen.handle_key(key).await;
                                }
                                AppScreen::Generator => {
                                    self.generator_screen.handle_key(key).await;
                                }
                                AppScreen::Models => {
                                    self.models_screen.handle_key(key).await;
                                }
                            }
                        }
                    }
                }
            }

            if self.should_quit {
                break;
            }

            // Update tunnel status periodically
            self.update_tunnel_status().await;
        }

        Ok(())
    }

    /// Draw the UI.
    fn draw(&mut self, f: &mut ratatui::Frame) {
        use ratatui::layout::{Constraint, Direction, Layout};
        use ratatui::style::{Color, Modifier, Style};
        use ratatui::widgets::{Block, Borders, Tabs, Paragraph};
        use ratatui::text::{Line, Span};

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // Tab bar
                Constraint::Min(0),     // Main content
                Constraint::Length(1),  // Status bar
            ])
            .split(f.area());

        // Tab bar
        let titles: Vec<Line> = ["Browser", "Generator", "Models"]
            .iter()
            .map(|t| Line::from(*t))
            .collect();
        let selected = match self.current_screen {
            AppScreen::Browser => 0,
            AppScreen::Generator => 1,
            AppScreen::Models => 2,
        };
        let tabs = Tabs::new(titles)
            .block(Block::default().borders(Borders::ALL).title("JWST Cosmos"))
            .select(selected)
            .style(Style::default().fg(Color::White))
            .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
        f.render_widget(tabs, chunks[0]);

        // Main content area
        match self.current_screen {
            AppScreen::Browser => self.browser_screen.draw(f, chunks[1]),
            AppScreen::Generator => self.generator_screen.draw(f, chunks[1]),
            AppScreen::Models => self.models_screen.draw(f, chunks[1]),
        }

        // Status bar
        let tunnel_indicator = if self.tunnel_status.ollama && self.tunnel_status.comfyui {
            Span::styled("🔗 Connected", Style::default().fg(Color::Green))
        } else if self.tunnel_status.ollama || self.tunnel_status.comfyui {
            Span::styled("🔗 Partial", Style::default().fg(Color::Yellow))
        } else {
            Span::styled("🔗 Disconnected", Style::default().fg(Color::Red))
        };

        let status = Paragraph::new(Line::from(vec![
            Span::raw(" "),
            Span::styled(&self.status_message, Style::default().fg(Color::Gray)),
            Span::raw(" │ "),
            tunnel_indicator,
            Span::raw(" │ "),
            Span::styled("Tab", Style::default().fg(Color::DarkGray)),
            Span::styled(" Switch", Style::default().fg(Color::Gray)),
            Span::raw(" │ "),
            Span::styled("[T]", Style::default().fg(Color::DarkGray)),
            Span::styled(" Tunnel", Style::default().fg(Color::Gray)),
            Span::raw(" │ "),
            Span::styled("[Q]", Style::default().fg(Color::DarkGray)),
            Span::styled(" Quit", Style::default().fg(Color::Gray)),
        ]));
        f.render_widget(status, chunks[2]);
    }

    /// Toggle SSH tunnels.
    async fn toggle_tunnels(&mut self) {
        let mut manager = self.tunnel_manager.lock().await;

        if self.tunnel_status.ollama || self.tunnel_status.comfyui {
            // Close tunnels
            manager.close_all();
            self.tunnel_status = TunnelStatus::default();
            self.status_message = "Tunnels closed".to_string();
        } else {
            // Open tunnels
            self.status_message = "Connecting to remote...".to_string();

            match manager.get_ollama_tunnel().await {
                Ok(url) => {
                    self.ollama_service.set_base_url(url).await;
                    self.tunnel_status.ollama = true;
                }
                Err(e) => {
                    self.status_message = format!("Ollama tunnel failed: {}", e);
                    return;
                }
            }

            match manager.get_comfyui_tunnel().await {
                Ok(url) => {
                    self.comfyui_service.set_base_url(url).await;
                    self.tunnel_status.comfyui = true;
                }
                Err(e) => {
                    self.status_message = format!("ComfyUI tunnel failed: {}", e);
                    return;
                }
            }

            self.status_message = "Connected to remote services".to_string();
        }
    }

    /// Update tunnel status.
    async fn update_tunnel_status(&mut self) {
        let mut manager = self.tunnel_manager.lock().await;
        self.tunnel_status.ollama = manager.is_tunnel_active("ollama");
        self.tunnel_status.comfyui = manager.is_tunnel_active("comfyui");
    }
}
