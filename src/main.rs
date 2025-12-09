//! JWST Cosmos - Space Image Browser and AI Image Generator TUI
//!
//! A terminal-based application for browsing James Webb Space Telescope images
//! and generating AI-enhanced wallpapers using remote ComfyUI and Ollama servers.

mod app;
mod config;
mod screens;
mod services;
mod utils;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// JWST Cosmos - Space Image Browser and AI Image Generator
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,

    /// Config file path (default: ~/.config/jwst-cosmos/config.toml)
    #[arg(short, long)]
    config: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Set up logging
    let filter = if args.debug {
        "jwst_cosmos=debug,info"
    } else {
        "jwst_cosmos=info,warn"
    };

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| filter.into()))
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    // Load configuration
    let config = if let Some(path) = args.config {
        config::Config::from_file(&path)?
    } else {
        config::Config::load()?
    };

    // Run the TUI application
    let mut app = app::App::new(config)?;
    app.run().await?;

    Ok(())
}
