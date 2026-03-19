use std::sync::Arc;

use tokio::process::Command;
use tracing::{debug, info, warn};

use sdk::errors::EngineError;

use super::super::sandbox::McpSandbox;
use super::{McpServerInstance, McpSpawner, MAX_RESTART_ATTEMPTS};

impl McpSpawner {
    pub async fn start_server(&self, name: &str) -> Result<(), EngineError> {
        {
            let servers = self.servers.read().await;
            if servers.contains_key(name) {
                debug!(server = name, "MCP server already running");
                return Ok(());
            }
        }

        let config = self
            .configs
            .get(name)
            .ok_or_else(|| EngineError::Plugin(format!("unknown MCP server: {}", name)))?;

        info!(server = name, "Starting MCP server");

        let command = McpSandbox::wrap_command(&config.command, &config.args, &config.profile)?;
        let mut command = Command::from(command);
        command.stdin(std::process::Stdio::piped());
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());

        let mut child = command.spawn().map_err(EngineError::Io)?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| EngineError::Plugin("failed to capture MCP server stdin".to_string()))?;
        let stdout = child.stdout.take().ok_or_else(|| {
            EngineError::Plugin("failed to capture MCP server stdout".to_string())
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            EngineError::Plugin("failed to capture MCP server stderr".to_string())
        })?;

        let instance = McpServerInstance {
            config: config.clone(),
            process: child,
            stdin,
            stdout: tokio::io::BufReader::new(stdout),
            stderr: tokio::io::BufReader::new(stderr),
            crash_count: 0,
            last_activity: std::time::Instant::now(),
        };

        self.servers
            .write()
            .await
            .insert(name.to_string(), instance);
        if let Err(error) = self.initialize_server(name).await {
            let _ = self.stop_server(name).await;
            return Err(error);
        }

        info!(server = name, "MCP server started successfully");
        Ok(())
    }

    pub async fn stop_server(&self, name: &str) -> Result<(), EngineError> {
        let mut servers = self.servers.write().await;
        if let Some(mut instance) = servers.remove(name) {
            info!(server = name, "Stopping MCP server");
            if let Err(error) = instance.process.kill().await {
                warn!(server = name, error = %error, "Failed to kill MCP server process");
            }
            let _ = instance.process.wait().await;
        }
        Ok(())
    }

    pub async fn stop_all(&self) {
        let server_names: Vec<String> = self.servers.read().await.keys().cloned().collect();
        for name in server_names {
            if let Err(error) = self.stop_server(&name).await {
                warn!(server = name, error = %error, "Failed to stop MCP server");
            }
        }
    }

    pub async fn is_running(&self, name: &str) -> bool {
        self.servers.read().await.contains_key(name)
    }

    pub fn configured_servers(&self) -> Vec<String> {
        self.configs.keys().cloned().collect()
    }

    pub async fn running_servers(&self) -> Vec<String> {
        self.servers.read().await.keys().cloned().collect()
    }

    pub async fn keepalive_loop(self: Arc<Self>) {
        const KEEPALIVE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);
        const IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

        loop {
            tokio::time::sleep(KEEPALIVE_INTERVAL).await;
            let server_names: Vec<String> = self.servers.read().await.keys().cloned().collect();

            for name in server_names {
                let mut servers = self.servers.write().await;
                if let Some(instance) = servers.get_mut(&name) {
                    let idle_time = instance.last_activity.elapsed();
                    if idle_time <= IDLE_TIMEOUT {
                        continue;
                    }

                    info!(
                        server = name,
                        idle_secs = idle_time.as_secs(),
                        "MCP server idle, checking health"
                    );

                    match instance.process.try_wait() {
                        Ok(Some(status)) => {
                            warn!(server = name, status = ?status, "MCP server process exited");
                            instance.crash_count += 1;
                            let crash_count = instance.crash_count;
                            drop(servers);

                            if crash_count < MAX_RESTART_ATTEMPTS {
                                info!(server = name, "Restarting crashed MCP server");
                                let _ = self.stop_server(&name).await;
                                if let Err(error) = self.start_server(&name).await {
                                    warn!(server = name, error = %error, "Failed to restart MCP server");
                                }
                            } else {
                                warn!(
                                    server = name,
                                    crashes = crash_count,
                                    "MCP server crashed too many times, not restarting"
                                );
                            }
                        }
                        Ok(None) => {
                            debug!(server = name, "MCP server process still alive");
                        }
                        Err(error) => {
                            warn!(server = name, error = %error, "Failed to check MCP server status");
                        }
                    }
                }
            }
        }
    }
}
