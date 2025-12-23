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

use crate::config::Config;
use crate::screens::{Screen, BrowserScreen, GeneratorScreen, ServicesScreen, ConnectionMode};
use crate::services::{
    EsaService, JwstApiService, TunnelManager, OllamaService, ComfyUiService, WallustService,
};

/// Application state.
pub struct App {
    current_screen: AppScreen,
    should_quit: bool,

    // Screens
    browser_screen: BrowserScreen,
    generator_screen: GeneratorScreen,
    services_screen: ServicesScreen,

    // Status bar info
    status_message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppScreen {
    Browser,
    Generator,
    Services,
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
        let services_screen = ServicesScreen::new(
            config.clone(),
            ollama_service.clone(),
            comfyui_service.clone(),
            tunnel_manager.clone(),
        );

        Ok(Self {
            current_screen: AppScreen::Browser,
            should_quit: false,
            browser_screen,
            generator_screen,
            services_screen,
            status_message: "Ready".to_string(),
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

        // Clean up spawned processes
        self.services_screen.stop_ollama_serve();

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
                    // Check if generator is in text input mode
                    let generator_editing = self.current_screen == AppScreen::Generator
                        && self.generator_screen.is_editing_prompt();

                    // Global key handlers (skip most when editing text)
                    match (key.modifiers, key.code) {
                        (KeyModifiers::CONTROL, KeyCode::Char('c')) |
                        (KeyModifiers::CONTROL, KeyCode::Char('q')) => {
                            self.should_quit = true;
                        }
                        (_, KeyCode::Char('q')) if self.current_screen == AppScreen::Browser => {
                            self.should_quit = true;
                        }
                        (_, KeyCode::Esc) if generator_editing => {
                            // Exit text editing mode
                            self.generator_screen.exit_prompt_editing();
                        }
                        (_, KeyCode::Tab) if !generator_editing => {
                            // Cycle to next tab (only when not editing)
                            self.current_screen = match self.current_screen {
                                AppScreen::Browser => AppScreen::Generator,
                                AppScreen::Generator => AppScreen::Services,
                                AppScreen::Services => AppScreen::Browser,
                            };

                            // Sync selected image and load models when switching TO generator
                            if self.current_screen == AppScreen::Generator {
                                if let Some(path) = self.browser_screen.get_reference_image_path() {
                                    self.generator_screen.set_reference_image(path.to_string_lossy().to_string());
                                }
                                self.generator_screen.ensure_models_loaded().await;
                            }
                        }
                        (KeyModifiers::SHIFT, KeyCode::BackTab) if !generator_editing => {
                            // Cycle to previous tab (Shift+Tab, only when not editing)
                            self.current_screen = match self.current_screen {
                                AppScreen::Browser => AppScreen::Services,
                                AppScreen::Generator => AppScreen::Browser,
                                AppScreen::Services => AppScreen::Generator,
                            };

                            // Sync selected image and load models when switching TO generator
                            if self.current_screen == AppScreen::Generator {
                                if let Some(path) = self.browser_screen.get_reference_image_path() {
                                    self.generator_screen.set_reference_image(path.to_string_lossy().to_string());
                                }
                                self.generator_screen.ensure_models_loaded().await;
                            }
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
                                AppScreen::Services => {
                                    self.services_screen.handle_key(key).await;
                                }
                            }
                        }
                    }
                }
            }

            if self.should_quit {
                break;
            }

            // Poll for generation progress updates
            if self.generator_screen.is_generating() {
                self.generator_screen.poll_progress().await;
            }

            // Update services status periodically
            self.services_screen.update_status().await;
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
        let titles: Vec<Line> = ["Browser", "Generator", "Services"]
            .iter()
            .map(|t| Line::from(*t))
            .collect();
        let selected = match self.current_screen {
            AppScreen::Browser => 0,
            AppScreen::Generator => 1,
            AppScreen::Services => 2,
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
            AppScreen::Services => self.services_screen.draw(f, chunks[1]),
        }

        // Status bar
        let svc_status = self.services_screen.status();
        let connection_indicator = match svc_status.mode {
            ConnectionMode::Local => {
                if svc_status.ollama_connected && svc_status.comfyui_connected {
                    Span::styled("🏠 Local", Style::default().fg(Color::Green))
                } else if svc_status.ollama_connected || svc_status.comfyui_connected {
                    Span::styled("🏠 Local (partial)", Style::default().fg(Color::Yellow))
                } else {
                    Span::styled("🏠 Local (failed)", Style::default().fg(Color::Red))
                }
            }
            ConnectionMode::Remote => {
                if svc_status.ollama_connected && svc_status.comfyui_connected {
                    Span::styled("🔗 Remote", Style::default().fg(Color::Green))
                } else if svc_status.ollama_connected || svc_status.comfyui_connected {
                    Span::styled("🔗 Remote (partial)", Style::default().fg(Color::Yellow))
                } else {
                    Span::styled("🔗 Remote (failed)", Style::default().fg(Color::Red))
                }
            }
            ConnectionMode::RemoteDirect => {
                if svc_status.comfyui_connected {
                    Span::styled("🌐 Remote (HTTPS)", Style::default().fg(Color::Green))
                } else {
                    Span::styled("🌐 Remote (HTTPS, failed)", Style::default().fg(Color::Red))
                }
            }
            ConnectionMode::Disconnected => {
                Span::styled("⭘ Disconnected", Style::default().fg(Color::DarkGray))
            }
        };

        // Ollama server indicator
        let ollama_server_indicator = if svc_status.ollama_serving {
            Span::styled("▶ Ollama", Style::default().fg(Color::Green))
        } else {
            Span::styled("■ Ollama", Style::default().fg(Color::DarkGray))
        };

        let status = Paragraph::new(Line::from(vec![
            Span::raw(" "),
            Span::styled(&self.status_message, Style::default().fg(Color::Gray)),
            Span::raw(" │ "),
            ollama_server_indicator,
            Span::raw(" │ "),
            connection_indicator,
            Span::raw(" │ "),
            Span::styled("Tab", Style::default().fg(Color::DarkGray)),
            Span::styled(" Switch", Style::default().fg(Color::Gray)),
            Span::raw(" │ "),
            Span::styled("[Q]", Style::default().fg(Color::DarkGray)),
            Span::styled(" Quit", Style::default().fg(Color::Gray)),
        ]));
        f.render_widget(status, chunks[2]);
    }
}
