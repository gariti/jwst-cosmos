//! Services management screen.

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

use super::Screen;
use crate::config::Config;
use crate::services::{ComfyUiService, OllamaService, TunnelManager};

/// Connection mode for services.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectionMode {
    #[default]
    Disconnected,
    Local,
    Remote,
    RemoteDirect,
}

/// Service connection status.
#[derive(Debug, Clone, Default)]
pub struct ServiceStatus {
    pub ollama_connected: bool,
    pub comfyui_connected: bool,
    pub mode: ConnectionMode,
    pub ollama_serving: bool,
}

/// Menu item in services screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuItem {
    ToggleOllama,
    ConnectLocal,
    ConnectRemote,
    ConnectRemoteDirect,
    Disconnect,
}

impl MenuItem {
    fn all() -> Vec<MenuItem> {
        vec![
            MenuItem::ToggleOllama,
            MenuItem::ConnectLocal,
            MenuItem::ConnectRemote,
            MenuItem::ConnectRemoteDirect,
            MenuItem::Disconnect,
        ]
    }

    fn label(&self) -> &'static str {
        match self {
            MenuItem::ToggleOllama => "Start Ollama Service",
            MenuItem::ConnectLocal => "Connect to Local Services",
            MenuItem::ConnectRemote => "Connect to Remote Services (SSH)",
            MenuItem::ConnectRemoteDirect => "Connect to Remote ComfyUI (HTTPS)",
            MenuItem::Disconnect => "Disconnect",
        }
    }
}

/// Services screen state.
pub struct ServicesScreen {
    config: Arc<Config>,
    ollama_service: Arc<OllamaService>,
    comfyui_service: Arc<ComfyUiService>,
    tunnel_manager: Arc<Mutex<TunnelManager>>,

    // UI state
    list_state: ListState,
    status: ServiceStatus,
    status_message: String,
}

impl ServicesScreen {
    pub fn new(
        config: Arc<Config>,
        ollama_service: Arc<OllamaService>,
        comfyui_service: Arc<ComfyUiService>,
        tunnel_manager: Arc<Mutex<TunnelManager>>,
    ) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));

        Self {
            config,
            ollama_service,
            comfyui_service,
            tunnel_manager,
            list_state,
            status: ServiceStatus::default(),
            status_message: String::new(),
        }
    }

    /// Get current service status.
    pub fn status(&self) -> &ServiceStatus {
        &self.status
    }

    /// Move selection up.
    fn previous(&mut self) {
        let count = MenuItem::all().len();
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
        let count = MenuItem::all().len();
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

    /// Execute the selected menu item.
    async fn execute_selected(&mut self) {
        let items = MenuItem::all();
        if let Some(idx) = self.list_state.selected() {
            if let Some(item) = items.get(idx) {
                match item {
                    MenuItem::ToggleOllama => self.toggle_ollama_service().await,
                    MenuItem::ConnectLocal => self.connect_local().await,
                    MenuItem::ConnectRemote => self.connect_remote().await,
                    MenuItem::ConnectRemoteDirect => self.connect_remote_direct().await,
                    MenuItem::Disconnect => self.disconnect().await,
                }
            }
        }
    }

    /// Run a shell command and return success status.
    fn run_command(cmd: &str) -> Result<(), String> {
        if cmd.is_empty() {
            return Ok(());
        }

        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .output()
            .map_err(|e| format!("Failed to execute: {}", e))?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(stderr.trim().to_string())
        }
    }

    /// Toggle Ollama service using configured commands.
    async fn toggle_ollama_service(&mut self) {
        if self.status.ollama_serving {
            // Stop the service
            self.status_message = "Stopping Ollama service...".to_string();
            let stop_cmd = self.config.remote.ollama_stop_cmd.clone();

            match Self::run_command(&stop_cmd) {
                Ok(_) => {
                    self.status.ollama_serving = false;
                    self.status_message = "Ollama service stopped".to_string();
                }
                Err(e) => {
                    self.status_message = format!("Failed to stop Ollama: {}", e);
                }
            }
        } else {
            // Start the service
            self.status_message = "Starting Ollama service...".to_string();
            let start_cmd = self.config.remote.ollama_start_cmd.clone();

            match Self::run_command(&start_cmd) {
                Ok(_) => {
                    self.status.ollama_serving = true;
                    self.status_message = "Ollama service started".to_string();

                    // Give it a moment to start, then auto-connect
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                    self.connect_local().await;
                }
                Err(e) => {
                    self.status_message = format!("Failed to start Ollama: {}", e);
                }
            }
        }
    }

    /// Stop Ollama service (called on app exit).
    pub fn stop_ollama_serve(&mut self) {
        if self.status.ollama_serving {
            let stop_cmd = &self.config.remote.ollama_stop_cmd;
            let _ = Self::run_command(stop_cmd);
            self.status.ollama_serving = false;
        }
    }

    /// Connect to local services.
    async fn connect_local(&mut self) {
        // Disconnect first if connected
        if self.status.mode != ConnectionMode::Disconnected {
            self.disconnect().await;
        }

        self.status_message = "Connecting to local services...".to_string();
        self.status.mode = ConnectionMode::Local;

        // Set local Ollama URL
        let ollama_url = format!("http://127.0.0.1:{}", self.config.remote.ollama_port);
        self.ollama_service.set_base_url(ollama_url).await;

        // Check if Ollama is running
        if self.ollama_service.is_connected().await {
            self.status.ollama_connected = true;
        }

        // Set local ComfyUI URL
        let comfyui_url = format!("http://127.0.0.1:{}", self.config.remote.comfyui_port);
        self.comfyui_service.set_base_url(comfyui_url).await;

        // Check if ComfyUI is running
        if self.comfyui_service.is_connected().await {
            self.status.comfyui_connected = true;
        }

        // Update status message
        if self.status.ollama_connected && self.status.comfyui_connected {
            self.status_message = "Connected to local services".to_string();
        } else if self.status.ollama_connected {
            self.status_message = "Connected to local Ollama (ComfyUI unavailable)".to_string();
        } else if self.status.comfyui_connected {
            self.status_message = "Connected to local ComfyUI (Ollama unavailable)".to_string();
        } else {
            self.status_message = "No local services available".to_string();
            self.status.mode = ConnectionMode::Disconnected;
        }
    }

    /// Connect to remote services via SSH tunnel.
    async fn connect_remote(&mut self) {
        // Disconnect first if connected
        if self.status.mode != ConnectionMode::Disconnected {
            self.disconnect().await;
        }

        self.status_message = "Connecting to remote services...".to_string();
        self.status.mode = ConnectionMode::Remote;

        let mut manager = self.tunnel_manager.lock().await;

        match manager.get_ollama_tunnel().await {
            Ok(url) => {
                self.ollama_service.set_base_url(url).await;
                self.status.ollama_connected = true;
            }
            Err(e) => {
                self.status_message = format!("Ollama tunnel failed: {}", e);
                self.status.mode = ConnectionMode::Disconnected;
                return;
            }
        }

        match manager.get_comfyui_tunnel().await {
            Ok(url) => {
                self.comfyui_service.set_base_url(url).await;
                self.status.comfyui_connected = true;
            }
            Err(e) => {
                self.status_message = format!("ComfyUI tunnel failed: {}", e);
                return;
            }
        }

        self.status_message = "Connected to remote services".to_string();
    }

    /// Connect to remote ComfyUI via direct HTTPS.
    async fn connect_remote_direct(&mut self) {
        // Disconnect first if connected
        if self.status.mode != ConnectionMode::Disconnected {
            self.disconnect().await;
        }

        // Check if remote URL is configured
        let url = match &self.config.comfyui_remote.url {
            Some(url) => url.clone(),
            None => {
                self.status_message = "No remote ComfyUI URL configured".to_string();
                return;
            }
        };

        // Get API key
        let api_key = self.config.comfyui_api_key();

        self.status_message = "Connecting to remote ComfyUI...".to_string();
        self.status.mode = ConnectionMode::RemoteDirect;

        // Set URL and API key
        self.comfyui_service.set_base_url(url).await;
        self.comfyui_service.set_api_key(api_key).await;

        // Test connection
        if self.comfyui_service.is_connected().await {
            self.status.comfyui_connected = true;
            self.status_message = "Connected to remote ComfyUI".to_string();
        } else {
            self.status.comfyui_connected = false;
            self.status.mode = ConnectionMode::Disconnected;
            self.status_message = "Failed to connect to remote ComfyUI".to_string();
        }
    }

    /// Disconnect from all services.
    async fn disconnect(&mut self) {
        let mut manager = self.tunnel_manager.lock().await;
        manager.close_all();

        // Clear API key when disconnecting
        self.comfyui_service.set_api_key(None).await;

        self.status.ollama_connected = false;
        self.status.comfyui_connected = false;
        self.status.mode = ConnectionMode::Disconnected;
        self.status_message = "Disconnected".to_string();
    }

    /// Update connection status (called periodically from app).
    pub async fn update_status(&mut self) {
        if self.status.mode == ConnectionMode::Remote {
            let mut manager = self.tunnel_manager.lock().await;
            self.status.ollama_connected = manager.is_tunnel_active("ollama");
            self.status.comfyui_connected = manager.is_tunnel_active("comfyui");
        }
    }
}

#[async_trait]
impl Screen for ServicesScreen {
    fn draw(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6),  // Status panel
                Constraint::Min(0),     // Menu
                Constraint::Length(3),  // Help
            ])
            .split(area);

        // Status panel
        let mode_text = match self.status.mode {
            ConnectionMode::Disconnected => Span::styled("Disconnected", Style::default().fg(Color::DarkGray)),
            ConnectionMode::Local => Span::styled("Local", Style::default().fg(Color::Green)),
            ConnectionMode::Remote => Span::styled("Remote (SSH)", Style::default().fg(Color::Cyan)),
            ConnectionMode::RemoteDirect => Span::styled("Remote (HTTPS)", Style::default().fg(Color::Cyan)),
        };

        let ollama_server = if self.status.ollama_serving {
            Span::styled("Running", Style::default().fg(Color::Green))
        } else {
            Span::styled("Not running", Style::default().fg(Color::DarkGray))
        };

        let ollama_status = if self.status.ollama_connected {
            Span::styled("Connected", Style::default().fg(Color::Green))
        } else {
            Span::styled("Not connected", Style::default().fg(Color::DarkGray))
        };

        let comfyui_status = if self.status.comfyui_connected {
            Span::styled("Connected", Style::default().fg(Color::Green))
        } else {
            Span::styled("Not connected", Style::default().fg(Color::DarkGray))
        };

        let status_lines = vec![
            Line::from(vec![Span::raw("Connection Mode: "), mode_text]),
            Line::from(vec![Span::raw("Ollama Server:   "), ollama_server]),
            Line::from(vec![Span::raw("Ollama:          "), ollama_status]),
            Line::from(vec![Span::raw("ComfyUI:         "), comfyui_status]),
        ];

        let status_panel = Paragraph::new(status_lines)
            .block(Block::default().borders(Borders::ALL).title("Status"));
        f.render_widget(status_panel, chunks[0]);

        // Menu
        let items: Vec<ListItem> = MenuItem::all()
            .iter()
            .map(|item| {
                let style = match item {
                    MenuItem::ToggleOllama => {
                        if self.status.ollama_serving {
                            Style::default().fg(Color::Yellow)
                        } else {
                            Style::default().fg(Color::White)
                        }
                    }
                    MenuItem::ConnectLocal | MenuItem::ConnectRemote | MenuItem::ConnectRemoteDirect => {
                        if self.status.mode != ConnectionMode::Disconnected {
                            Style::default().fg(Color::DarkGray)
                        } else {
                            Style::default().fg(Color::White)
                        }
                    }
                    MenuItem::Disconnect => {
                        if self.status.mode == ConnectionMode::Disconnected {
                            Style::default().fg(Color::DarkGray)
                        } else {
                            Style::default().fg(Color::White)
                        }
                    }
                };

                let label = match item {
                    MenuItem::ToggleOllama if self.status.ollama_serving => "Stop Ollama Service",
                    _ => item.label(),
                };

                ListItem::new(Line::from(Span::styled(label, style)))
            })
            .collect();

        let menu = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Actions"))
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("► ");

        f.render_stateful_widget(menu, chunks[1], &mut self.list_state);

        // Help / Status message
        let help_text = if !self.status_message.is_empty() {
            Line::from(vec![
                Span::styled(&self.status_message, Style::default().fg(Color::Yellow)),
            ])
        } else {
            Line::from(vec![
                Span::styled("[↑/↓]", Style::default().fg(Color::DarkGray)),
                Span::raw(" Navigate "),
                Span::styled("[Enter]", Style::default().fg(Color::DarkGray)),
                Span::raw(" Select"),
            ])
        };

        let help = Paragraph::new(help_text)
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(help, chunks[2]);
    }

    async fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.previous(),
            KeyCode::Down | KeyCode::Char('j') => self.next(),
            KeyCode::Enter => self.execute_selected().await,
            _ => {}
        }
    }
}
