//! Integration tests for MCP + Gate 5
//!
//! Phase 4 Complete Criteria:
//! - At least one MCP server connected and working
//! - Gate 5 sandbox proven: MCP process cannot read host filesystem outside declaration
//! - InjectionDetector blocking simulated injection in MCP output

use rove_engine::mcp::{McpSandbox, McpServerConfig, McpSpawner, SandboxProfile};
use sdk::errors::EngineError;

#[tokio::test]
async fn test_mcp_spawner_rejects_unknown_server() {
    let spawner = McpSpawner::new(vec![]);
    let result = spawner.start_server("unknown-server").await;

    assert!(result.is_err());
    match result {
        Err(EngineError::Plugin(msg)) => {
            assert!(msg.contains("unknown MCP server"));
        }
        _ => panic!("Expected Plugin error for unknown server"),
    }
}

#[tokio::test]
async fn test_mcp_spawner_tracks_configured_servers() {
    let config = McpServerConfig {
        name: "test-server".to_string(),
        template: Some("custom".to_string()),
        description: Some("test server".to_string()),
        command: "echo".to_string(),
        args: vec!["hello".to_string()],
        profile: SandboxProfile::default(),
        cached_tools: Vec::new(),
        enabled: true,
    };

    let spawner = McpSpawner::new(vec![config]);
    let configured = spawner.configured_servers();

    assert_eq!(configured.len(), 1);
    assert_eq!(configured[0], "test-server");
}

#[tokio::test]
async fn test_mcp_spawner_filters_disabled_servers() {
    let configs = vec![
        McpServerConfig {
            name: "enabled".to_string(),
            template: Some("custom".to_string()),
            description: Some("enabled".to_string()),
            command: "echo".to_string(),
            args: vec![],
            profile: SandboxProfile::default(),
            cached_tools: Vec::new(),
            enabled: true,
        },
        McpServerConfig {
            name: "disabled".to_string(),
            template: Some("custom".to_string()),
            description: Some("disabled".to_string()),
            command: "echo".to_string(),
            args: vec![],
            profile: SandboxProfile::default(),
            cached_tools: Vec::new(),
            enabled: false,
        },
    ];

    let spawner = McpSpawner::new(configs);
    let configured = spawner.configured_servers();

    assert_eq!(configured.len(), 1);
    assert_eq!(configured[0], "enabled");
}

#[test]
fn test_sandbox_profile_builder() {
    let profile = SandboxProfile::default()
        .with_network()
        .with_read_path("/usr/local")
        .with_write_path("/tmp/test")
        .with_tmp();

    assert!(profile.allow_network);
    assert_eq!(profile.read_paths.len(), 1);
    assert_eq!(profile.write_paths.len(), 1);
    assert!(profile.allow_tmp);
}

#[test]
fn test_sandbox_default_is_restrictive() {
    let profile = SandboxProfile::default();

    assert!(
        !profile.allow_network,
        "Default profile should deny network"
    );
    assert!(
        profile.read_paths.is_empty(),
        "Default profile should have no read paths"
    );
    assert!(
        profile.write_paths.is_empty(),
        "Default profile should have no write paths"
    );
    assert!(!profile.allow_tmp, "Default profile should deny /tmp");
}

#[cfg(target_os = "linux")]
#[test]
fn test_linux_sandbox_denies_network_by_default() {
    let profile = SandboxProfile::default();
    let result = McpSandbox::wrap_command("echo", &["test".to_string()], &profile);

    if let Ok(cmd) = result {
        let args: Vec<String> = cmd
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        assert!(
            args.contains(&"--unshare-net".to_string()),
            "Should isolate network by default"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn test_linux_sandbox_allows_network_when_configured() {
    let profile = SandboxProfile::default().with_network();
    let result = McpSandbox::wrap_command("echo", &["test".to_string()], &profile);

    if let Ok(cmd) = result {
        let args: Vec<String> = cmd
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        assert!(
            !args.contains(&"--unshare-net".to_string()),
            "Should allow network when configured"
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn test_macos_sandbox_uses_seatbelt() {
    let profile = SandboxProfile::default();
    let result = McpSandbox::wrap_command("echo", &["test".to_string()], &profile);

    assert!(result.is_ok());
    let cmd = result.unwrap();
    let program = cmd.get_program().to_string_lossy();
    assert_eq!(program, "sandbox-exec", "macOS should use sandbox-exec");
}

#[cfg(target_os = "windows")]
#[test]
fn test_windows_sandbox_creates_command() {
    let profile = SandboxProfile::default();
    let result =
        McpSandbox::wrap_command("cmd.exe", &["/c".to_string(), "echo".to_string()], &profile);

    assert!(
        result.is_ok(),
        "Windows sandbox should create command successfully"
    );
}
