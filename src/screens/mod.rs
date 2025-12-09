//! TUI screens.

pub mod browser;
pub mod generator;
pub mod models;

pub use browser::BrowserScreen;
pub use generator::GeneratorScreen;
pub use models::ModelsScreen;

use async_trait::async_trait;
use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::Frame;

/// Trait for screen implementations.
#[async_trait]
pub trait Screen {
    /// Draw the screen.
    fn draw(&mut self, f: &mut Frame, area: Rect);

    /// Handle a key event.
    async fn handle_key(&mut self, key: KeyEvent);
}
