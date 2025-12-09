# JWST Cosmos 🌌

A Rust-based TUI (Terminal User Interface) for browsing James Webb Space Telescope images and generating AI-enhanced wallpapers.

![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)
![MIT License](https://img.shields.io/badge/license-MIT-green.svg?style=for-the-badge)

## Features

- **🔭 JWST Image Browser**: Browse recent images from ESA/Webb Gallery (RSS feed) and JWST API
- **🎨 AI Image Generation**: Transform space images using img2img and ControlNet techniques
- **🤖 Remote Model Management**: Manage Ollama and ComfyUI models on remote servers
- **🔗 SSH Tunneling**: Secure connection to remote GPU servers for AI processing
- **🎭 Wallust Integration**: Automatic wallpaper application with theme synchronization

## Installation

### From Source

```bash
cargo install --path .
```

### NixOS

Add to your flake inputs:

```nix
{
  inputs.jwst-cosmos.url = "path:/etc/nixos/jwst-cosmos-rs";
}
```

Then add to your packages:

```nix
{
  home.packages = [ inputs.jwst-cosmos.packages.${pkgs.system}.default ];
}
```

## Usage

```bash
# Run the TUI
jwst-cosmos

# Enable debug logging
jwst-cosmos --debug

# Use custom config
jwst-cosmos --config /path/to/config.toml
```

### Key Bindings

| Key | Action |
|-----|--------|
| `b` | Switch to Browser screen |
| `g` | Switch to Generator screen |
| `m` | Switch to Models screen |
| `t` | Toggle SSH tunnels |
| `q` | Quit |

#### Browser Screen
| Key | Action |
|-----|--------|
| `↑/k` | Previous image |
| `↓/j` | Next image |
| `Enter` | Download selected image |
| `r` | Refresh image list |
| `s` | Toggle image source (ESA/API) |

#### Generator Screen
| Key | Action |
|-----|--------|
| `Tab` | Next field |
| `←/→` | Cycle options |
| `Enter` | Start generation |
| `Esc` | Cancel generation |

#### Models Screen
| Key | Action |
|-----|--------|
| `Tab` | Switch between Ollama/ComfyUI |
| `p` | Pull new model |
| `d` | Delete selected model |
| `r` | Refresh model list |

## Configuration

Create `~/.config/jwst-cosmos/config.toml`:

```toml
[jwst]
api_key_file = "/run/agenix/jwst-api-key"
wallpaper_dir = "~/Pictures/Wallpapers"
cache_dir = "~/.cache/jwst-cosmos"
cache_ttl = 3600

[remote]
host = "192.168.0.27"
user = "garrett"
ollama_port = 11434
comfyui_port = 8188

[generation]
default_size = "5120x2160"
default_model = "sdxl"
enable_upscaling = true

[wallust]
auto_apply = true
refresh_script = "/etc/nixos/scripts/refresh-theme"
```

## Generation Modes

### img2img
Transform JWST images while preserving their cosmic essence.

### ControlNet Depth
Use depth estimation to maintain structural composition while applying new styles.

### ControlNet Canny
Preserve edge details from the original image for precise style transfer.

## Size Presets

- **HD**: 1920x1080
- **QHD**: 2560x1440
- **Laptop**: 2560x1600
- **4K UHD**: 3840x2160
- **Ultrawide**: 5120x2160

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      JWST Cosmos TUI                        │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │   Browser   │  │  Generator  │  │       Models        │  │
│  │  (ESA/API)  │  │ (ComfyUI)   │  │ (Ollama/ComfyUI)    │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
│         │                │                    │             │
│  ┌──────┴────────────────┴────────────────────┴──────┐      │
│  │              Service Layer                        │      │
│  │  EsaService │ JwstApiService │ OllamaService │   │      │
│  │  ComfyUiService │ SshTunnel │ WallustService │   │      │
│  └───────────────────────────────────────────────────┘      │
└─────────────────────────────────────────────────────────────┘
                            │
                      SSH Tunnel
                            │
                            ▼
              ┌─────────────────────────┐
              │   Remote GPU Server     │
              │  ┌─────────┐ ┌───────┐  │
              │  │ Ollama  │ │ComfyUI│  │
              │  │ :11434  │ │ :8188 │  │
              │  └─────────┘ └───────┘  │
              │      NVIDIA 4080        │
              └─────────────────────────┘
```

## Dependencies

- Rust 1.70+
- SSH (for tunnel management)
- Remote server with:
  - Ollama with vision models (e.g., llava, moondream)
  - ComfyUI with SDXL/Flux checkpoints

## License

MIT License - see [LICENSE](LICENSE) for details.

## Acknowledgments

- [ESA/Webb](https://esawebb.org/) for the stunning imagery
- [JWST API](https://jwstapi.com/) for API access
- [Ratatui](https://ratatui.rs/) for the TUI framework
- [ComfyUI](https://github.com/comfyanonymous/ComfyUI) for image generation
- [Ollama](https://ollama.ai/) for local LLM inference
