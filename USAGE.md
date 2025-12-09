# JWST Cosmos - Usage Guide

## Quick Start

### Running the App

```bash
# Run the Nix-built version (stable, from GitHub)
cosmos

# Or use the alias
jwst

# Run your local development build
cosmos-dev
```

### Development Workflow

```bash
# Go to the project
cd ~/Projects/jwst-cosmos

# Build debug version (faster compile, slower runtime)
cargo build

# Build release version (slower compile, optimized)
cargo build --release

# Run directly with cargo
cargo run

# Run with debug logging
cargo run -- --debug

# Watch for changes and rebuild automatically
cargo watch -x build
```

### Rofi Launchers

**TUI Launcher** (your TUI apps menu):
- Look for `󰖥 jwst-cosmos` in the list
- Launches the Nix-built version in a terminal

**Lazygit Menu**:
- Select "JWST Cosmos" to open lazygit in `~/Projects/jwst-cosmos`

**Claude Launcher**:
- Select "󰖥 JWST Cosmos" as the repo
- Then choose to resume, start new instance, or enter a task prompt

## Deploying Changes

After making changes to the code:

```bash
cd ~/Projects/jwst-cosmos

# Test locally first
cargo build --release
cosmos-dev  # Test your changes

# Commit and push
git add .
git commit -m "Your commit message"
git push

# Update NixOS to use the new version
cd /etc/nixos
nix flake update jwst-cosmos
nrs  # Rebuild NixOS
```

## Key Bindings (In the TUI)

### Global
| Key | Action |
|-----|--------|
| `b` | Switch to Browser screen |
| `g` | Switch to Generator screen |
| `m` | Switch to Models screen |
| `t` | Toggle SSH tunnels |
| `q` | Quit |

### Browser Screen
| Key | Action |
|-----|--------|
| `↑/k` | Previous image |
| `↓/j` | Next image |
| `Enter` | Download selected image |
| `r` | Refresh image list |
| `s` | Toggle source (ESA/API) |

### Generator Screen
| Key | Action |
|-----|--------|
| `Tab` | Next field |
| `←/→` | Cycle options |
| `Enter` | Start generation |
| `Esc` | Cancel |

### Models Screen
| Key | Action |
|-----|--------|
| `Tab` | Switch Ollama/ComfyUI |
| `p` | Pull new model |
| `d` | Delete model |
| `r` | Refresh list |

## Configuration

Config file: `~/.config/jwst-cosmos/config.toml`

Key settings:
- `[jwst]` - API keys, directories, feed URLs
- `[remote]` - SSH host for GPU server (192.168.0.27)
- `[generation]` - Default sizes, models, upscaling
- `[wallust]` - Theme integration settings

## Troubleshooting

**SSH tunnels not connecting:**
```bash
# Check if remote server is reachable
ping 192.168.0.27

# Test SSH connection
ssh garrett@192.168.0.27
```

**API key issues:**
```bash
# Verify the key file exists
cat /run/agenix/jwst-api-key
```

**Colors look wrong:**
```bash
# Refresh wallust theme
refresh
```
