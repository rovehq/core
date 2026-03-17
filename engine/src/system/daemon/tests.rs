use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use super::{DaemonManager, Result};
use crate::config::Config;
use sdk::errors::EngineError;

fn create_test_config(temp_dir: &TempDir) -> Config {
    let config_path = temp_dir.path().join("config.toml");
    let workspace_str = temp_dir.path().to_string_lossy().replace('\\', "/");
    let config_content = format!(
        r#"
[core]
workspace = "{workspace}"
log_level = "info"
auto_sync = true
data_dir = "{workspace}"

[llm]
default_provider = "ollama"

[tools]
tg-controller = false
ui-server = false
api-server = false

[plugins]
fs-editor = true
terminal = true
screenshot = false
git = true

[security]
max_risk_tier = 2
confirm_tier1 = true
confirm_tier1_delay = 10
require_explicit_tier2 = true
"#,
        workspace = workspace_str
    );

    fs::write(&config_path, config_content).unwrap();
    Config::load_from_path(&config_path).unwrap()
}

fn create_safe_temp_dir() -> TempDir {
    let base = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    TempDir::new_in(base).unwrap()
}

#[tokio::test]
async fn test_daemon_manager_creation() {
    let temp_dir = create_safe_temp_dir();
    let config = create_test_config(&temp_dir);

    let manager = DaemonManager::new(&config).unwrap();
    assert!(manager.pid_file.to_string_lossy().contains("rove.pid"));
}

#[tokio::test]
async fn test_write_and_read_pid_file() {
    let temp_dir = create_safe_temp_dir();
    let config = create_test_config(&temp_dir);

    let manager = DaemonManager::new(&config).unwrap();
    manager.write_pid_file().unwrap();

    assert!(manager.pid_file.exists());

    let pid = DaemonManager::read_pid_file(&manager.pid_file).unwrap();
    assert_eq!(pid, std::process::id());
}

#[tokio::test]
#[cfg(unix)]
async fn test_daemon_already_running() {
    let temp_dir = create_safe_temp_dir();
    let config = create_test_config(&temp_dir);

    let manager = DaemonManager::new(&config).unwrap();
    manager.start().await.unwrap();

    let result: Result<()> = manager.start().await;
    assert!(matches!(result, Err(EngineError::DaemonAlreadyRunning)));
}

#[tokio::test]
async fn test_stale_pid_file_handling() {
    let temp_dir = create_safe_temp_dir();
    let config = create_test_config(&temp_dir);

    let manager = DaemonManager::new(&config).unwrap();
    fs::create_dir_all(manager.pid_file.parent().unwrap()).unwrap();
    fs::write(&manager.pid_file, "999999").unwrap();

    let result = manager.start().await;
    assert!(result.is_ok());
}

#[tokio::test]
#[cfg(unix)]
async fn test_daemon_status() {
    let temp_dir = create_safe_temp_dir();
    let config = create_test_config(&temp_dir);

    let status = DaemonManager::status(&config).unwrap();
    assert!(!status.is_running);
    assert!(status.pid.is_none());

    let manager = DaemonManager::new(&config).unwrap();
    manager.start().await.unwrap();

    let status = DaemonManager::status(&config).unwrap();
    assert!(status.is_running);
    assert_eq!(status.pid, Some(std::process::id()));
}

#[tokio::test]
async fn test_pid_file_cleanup_on_drop() {
    let temp_dir = create_safe_temp_dir();
    let config = create_test_config(&temp_dir);

    let pid_file = {
        let manager = DaemonManager::new(&config).unwrap();
        manager.write_pid_file().unwrap();
        assert!(manager.pid_file.exists());
        manager.pid_file.clone()
    };

    assert!(!pid_file.exists());
}

#[tokio::test]
async fn test_daemon_status_provider_availability() {
    let temp_dir = create_safe_temp_dir();
    let config = create_test_config(&temp_dir);
    let status = DaemonManager::status(&config).unwrap();

    let _openai = status.providers.openai;
    let _anthropic = status.providers.anthropic;
    let _gemini = status.providers.gemini;
    let _nvidia_nim = status.providers.nvidia_nim;
    let _ollama = status.providers.ollama;
}
