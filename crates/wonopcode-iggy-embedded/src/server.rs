//! Embedded Iggy server management.

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::oneshot;
use tracing::{debug, error, info, warn};

use crate::config::EmbeddedIggyConfig;
use crate::error::EmbeddedIggyError;

/// Handle to an embedded Iggy server.
///
/// The server is automatically stopped when this handle is dropped.
pub struct EmbeddedIggy {
    /// TCP address the server is listening on.
    tcp_address: String,
    /// HTTP address for health checks (if enabled).
    http_address: Option<String>,
    /// QUIC address (if enabled).
    quic_address: Option<String>,
    /// Server process handle.
    child: Child,
    /// Temporary directory (if using in-memory mode).
    _temp_dir: Option<tempfile::TempDir>,
    /// Shutdown signal sender.
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl EmbeddedIggy {
    /// Start an embedded Iggy server.
    ///
    /// This will:
    /// 1. Find available ports
    /// 2. Create a temporary data directory (if not specified)
    /// 3. Generate configuration
    /// 4. Start the iggy-server process
    /// 5. Wait for it to become healthy
    pub async fn start(config: EmbeddedIggyConfig) -> Result<Self, EmbeddedIggyError> {
        // Find the iggy-server binary
        let binary_path = Self::find_binary(&config)?;
        info!("Using iggy-server binary: {:?}", binary_path);

        // Allocate ports
        let tcp_port = Self::get_available_port(config.tcp_port)?;
        let http_port = if config.enable_http {
            Self::get_available_port(config.http_port)?
        } else {
            0
        };
        let quic_port = if config.enable_quic {
            Self::get_available_port(config.quic_port)?
        } else {
            0
        };

        info!(
            "Allocated ports - TCP: {}, HTTP: {}, QUIC: {}",
            tcp_port, http_port, quic_port
        );

        // Create data directory
        let (data_path, temp_dir) = match &config.data_dir {
            Some(dir) => {
                tokio::fs::create_dir_all(dir)
                    .await
                    .map_err(|e| EmbeddedIggyError::DataDirectoryError(e.to_string()))?;
                (dir.clone(), None)
            }
            None => {
                let temp = tempfile::tempdir()
                    .map_err(|e| EmbeddedIggyError::DataDirectoryError(e.to_string()))?;
                (temp.path().to_path_buf(), Some(temp))
            }
        };

        info!("Using data directory: {:?}", data_path);

        // Generate configuration file
        let config_content =
            config.to_toml(tcp_port, http_port, quic_port, data_path.to_str().unwrap_or("."));

        let config_path = data_path.join("server.toml");
        tokio::fs::write(&config_path, &config_content)
            .await
            .map_err(|e| EmbeddedIggyError::ConfigError(e.to_string()))?;

        debug!("Generated config at {:?}", config_path);

        // Start the server process
        let mut child = Command::new(&binary_path)
            .arg("--config-path")
            .arg(&config_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| EmbeddedIggyError::StartFailed(e.to_string()))?;

        info!("Started iggy-server process (PID: {:?})", child.id());

        // Create shutdown channel
        let (shutdown_tx, _shutdown_rx) = oneshot::channel();

        // Spawn task to monitor stdout/stderr
        if let Some(stdout) = child.stdout.take() {
            tokio::spawn(async move {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    debug!("[iggy-server] {}", line);
                }
            });
        }

        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if line.contains("ERROR") || line.contains("error") {
                        error!("[iggy-server] {}", line);
                    } else {
                        warn!("[iggy-server] {}", line);
                    }
                }
            });
        }

        // Wait for server to be ready
        let tcp_address = format!("127.0.0.1:{}", tcp_port);
        let http_address = if config.enable_http {
            Some(format!("127.0.0.1:{}", http_port))
        } else {
            None
        };
        let quic_address = if config.enable_quic {
            Some(format!("127.0.0.1:{}", quic_port))
        } else {
            None
        };

        Self::wait_for_ready(&tcp_address, config.startup_timeout_secs).await?;

        info!(
            "Iggy server is ready at TCP: {}, HTTP: {:?}",
            tcp_address, http_address
        );

        Ok(Self {
            tcp_address,
            http_address,
            quic_address,
            child,
            _temp_dir: temp_dir,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    /// Find the iggy-server binary.
    fn find_binary(config: &EmbeddedIggyConfig) -> Result<PathBuf, EmbeddedIggyError> {
        // First check if explicit path is provided
        if let Some(path) = &config.binary_path {
            if path.exists() {
                return Ok(path.clone());
            }
            return Err(EmbeddedIggyError::BinaryNotFound(format!(
                "Specified binary not found: {:?}",
                path
            )));
        }

        // Check common locations
        let candidates = vec![
            // Current directory
            PathBuf::from("iggy-server"),
            // Cargo target directory
            PathBuf::from("target/release/iggy-server"),
            PathBuf::from("target/debug/iggy-server"),
            // User's cargo bin
            dirs::home_dir()
                .map(|h| h.join(".cargo/bin/iggy-server"))
                .unwrap_or_default(),
        ];

        for candidate in candidates {
            if candidate.exists() {
                return Ok(candidate);
            }
        }

        // Try to find in PATH
        if let Ok(path) = which::which("iggy-server") {
            return Ok(path);
        }

        Err(EmbeddedIggyError::BinaryNotFound(
            "iggy-server not found. Install with: cargo install iggy".to_string(),
        ))
    }

    /// Get an available port, or use the specified port if non-zero.
    fn get_available_port(requested: u16) -> Result<u16, EmbeddedIggyError> {
        if requested != 0 {
            // Verify the requested port is available
            TcpListener::bind(format!("127.0.0.1:{}", requested))
                .map_err(|e| {
                    EmbeddedIggyError::PortBindingFailed(format!(
                        "Port {} not available: {}",
                        requested, e
                    ))
                })
                .map(|_| requested)
        } else {
            // Find a random available port
            let listener = TcpListener::bind("127.0.0.1:0")
                .map_err(|e| EmbeddedIggyError::PortBindingFailed(e.to_string()))?;
            let port = listener.local_addr()?.port();
            Ok(port)
        }
    }

    /// Wait for the server to become ready by attempting TCP connections.
    async fn wait_for_ready(address: &str, timeout_secs: u64) -> Result<(), EmbeddedIggyError> {
        let timeout = Duration::from_secs(timeout_secs);
        let start = std::time::Instant::now();
        let check_interval = Duration::from_millis(100);

        while start.elapsed() < timeout {
            match tokio::net::TcpStream::connect(address).await {
                Ok(_) => {
                    debug!("Server is accepting connections at {}", address);
                    // Give it a moment to fully initialize
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    return Ok(());
                }
                Err(_) => {
                    tokio::time::sleep(check_interval).await;
                }
            }
        }

        Err(EmbeddedIggyError::Timeout)
    }

    /// Get the TCP address the server is listening on.
    pub fn tcp_address(&self) -> &str {
        &self.tcp_address
    }

    /// Get the HTTP address (if HTTP is enabled).
    pub fn http_address(&self) -> Option<&str> {
        self.http_address.as_deref()
    }

    /// Get the QUIC address (if QUIC is enabled).
    pub fn quic_address(&self) -> Option<&str> {
        self.quic_address.as_deref()
    }

    /// Get a transport config configured for this embedded server.
    pub fn transport_config(&self) -> crate::TransportConfig {
        crate::TransportConfig::new(&self.tcp_address)
    }

    /// Check if the server is still running.
    pub fn is_running(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(None) => true,         // Still running
            Ok(Some(_)) => false,     // Exited
            Err(_) => false,          // Error checking status
        }
    }

    /// Stop the embedded server.
    pub async fn stop(mut self) -> Result<(), EmbeddedIggyError> {
        info!("Stopping embedded Iggy server");

        // Send shutdown signal
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }

        // Try graceful shutdown first
        #[cfg(unix)]
        {
            if let Some(pid) = self.child.id() {
                // SAFETY: We're sending SIGTERM to a process we own
                #[allow(unsafe_code)]
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
            }
        }

        // Wait for process to exit with timeout
        let timeout = Duration::from_secs(5);
        match tokio::time::timeout(timeout, self.child.wait()).await {
            Ok(Ok(status)) => {
                info!("Iggy server exited with status: {:?}", status);
                Ok(())
            }
            Ok(Err(e)) => {
                warn!("Error waiting for server exit: {}", e);
                // Force kill
                let _ = self.child.kill().await;
                Ok(())
            }
            Err(_) => {
                warn!("Server did not exit gracefully, force killing");
                let _ = self.child.kill().await;
                Ok(())
            }
        }
    }
}

impl Drop for EmbeddedIggy {
    fn drop(&mut self) {
        // Send shutdown signal if still active
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        // Child process will be killed due to kill_on_drop(true)
    }
}
