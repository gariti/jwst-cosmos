//! SSH tunnel management for remote service access.

use anyhow::{Context, Result};
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use crate::config::Config;

/// Represents an active SSH tunnel.
pub struct SshTunnel {
    process: Child,
    local_port: u16,
    remote_port: u16,
    remote_host: String,
}

impl SshTunnel {
    /// Create a new SSH tunnel.
    pub fn new(
        config: &Config,
        remote_port: u16,
        local_port: u16,
    ) -> Result<Self> {
        let remote_host = format!("{}@{}", config.remote.user, config.remote.host);

        let mut cmd = Command::new("ssh");
        cmd.args([
            "-N",  // Don't execute remote command
            "-L",  // Local port forwarding
            &format!("{}:localhost:{}", local_port, remote_port),
            "-o", "StrictHostKeyChecking=no",
            "-o", "UserKnownHostsFile=/dev/null",
            "-o", "ServerAliveInterval=30",
            "-o", "ServerAliveCountMax=3",
            "-o", "ExitOnForwardFailure=yes",
        ]);

        // Add SSH key if configured
        if let Some(key_path) = &config.remote.ssh_key {
            cmd.args(["-i", key_path]);
        }

        cmd.arg(&remote_host);
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());

        let process = cmd.spawn().context("Failed to start SSH tunnel")?;

        Ok(Self {
            process,
            local_port,
            remote_port,
            remote_host: config.remote.host.clone(),
        })
    }

    /// Get the local endpoint URL.
    pub fn local_url(&self) -> String {
        format!("http://localhost:{}", self.local_port)
    }

    /// Check if the tunnel process is still running.
    pub fn is_alive(&mut self) -> bool {
        match self.process.try_wait() {
            Ok(None) => true,  // Still running
            _ => false,         // Exited or error
        }
    }

    /// Wait for the tunnel to be ready by checking the local port.
    pub async fn wait_ready(&mut self, timeout_secs: u64) -> Result<()> {
        let start = std::time::Instant::now();
        let timeout = Duration::from_secs(timeout_secs);

        while start.elapsed() < timeout {
            // Check if process is still alive
            if !self.is_alive() {
                anyhow::bail!("SSH tunnel process died");
            }

            // Try to connect to local port
            if tokio::net::TcpStream::connect(format!("127.0.0.1:{}", self.local_port))
                .await
                .is_ok()
            {
                return Ok(());
            }

            sleep(Duration::from_millis(100)).await;
        }

        anyhow::bail!("Timeout waiting for SSH tunnel to be ready")
    }

    /// Kill the tunnel process.
    pub fn kill(&mut self) -> Result<()> {
        // Try graceful termination first
        let pid = self.process.id();
        let _ = signal::kill(Pid::from_raw(pid as i32), Signal::SIGTERM);

        // Wait a bit then force kill if needed
        std::thread::sleep(Duration::from_millis(100));
        let _ = self.process.kill();
        let _ = self.process.wait();

        Ok(())
    }
}

impl Drop for SshTunnel {
    fn drop(&mut self) {
        let _ = self.kill();
    }
}

/// Manager for multiple SSH tunnels.
pub struct TunnelManager {
    config: Arc<Config>,
    tunnels: HashMap<String, SshTunnel>,
    next_local_port: u16,
}

impl TunnelManager {
    /// Create a new tunnel manager.
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            config,
            tunnels: HashMap::new(),
            next_local_port: 19000,  // Start from a high port
        }
    }

    /// Get or create a tunnel for a specific service.
    pub async fn get_tunnel(&mut self, service_name: &str, remote_port: u16) -> Result<String> {
        // Check if we already have an active tunnel
        if let Some(tunnel) = self.tunnels.get_mut(service_name) {
            if tunnel.is_alive() {
                return Ok(tunnel.local_url());
            }
            // Tunnel died, remove it
            self.tunnels.remove(service_name);
        }

        // Create new tunnel
        let local_port = self.next_local_port;
        self.next_local_port += 1;

        let mut tunnel = SshTunnel::new(&self.config, remote_port, local_port)
            .context(format!("Failed to create tunnel for {}", service_name))?;

        // Wait for it to be ready
        tunnel.wait_ready(10).await
            .context(format!("Tunnel for {} not ready", service_name))?;

        let url = tunnel.local_url();
        self.tunnels.insert(service_name.to_string(), tunnel);

        Ok(url)
    }

    /// Get a tunnel for Ollama.
    pub async fn get_ollama_tunnel(&mut self) -> Result<String> {
        self.get_tunnel("ollama", self.config.remote.ollama_port).await
    }

    /// Get a tunnel for ComfyUI.
    pub async fn get_comfyui_tunnel(&mut self) -> Result<String> {
        self.get_tunnel("comfyui", self.config.remote.comfyui_port).await
    }

    /// Check if a service tunnel is active.
    pub fn is_tunnel_active(&mut self, service_name: &str) -> bool {
        if let Some(tunnel) = self.tunnels.get_mut(service_name) {
            tunnel.is_alive()
        } else {
            false
        }
    }

    /// Close a specific tunnel.
    pub fn close_tunnel(&mut self, service_name: &str) -> Result<()> {
        if let Some(mut tunnel) = self.tunnels.remove(service_name) {
            tunnel.kill()?;
        }
        Ok(())
    }

    /// Close all tunnels.
    pub fn close_all(&mut self) {
        for (_, mut tunnel) in self.tunnels.drain() {
            let _ = tunnel.kill();
        }
    }

    /// Get tunnel status summary.
    pub fn status(&mut self) -> Vec<(String, bool)> {
        self.tunnels
            .iter_mut()
            .map(|(name, tunnel)| (name.clone(), tunnel.is_alive()))
            .collect()
    }
}

impl Drop for TunnelManager {
    fn drop(&mut self) {
        self.close_all();
    }
}
