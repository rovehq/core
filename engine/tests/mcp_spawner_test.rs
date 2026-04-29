use std::sync::Arc;

use rove_engine::runtime::mcp::SandboxProfile;
use rove_engine::runtime::mcp::{McpServer, McpServerConfig, McpSpawner};

#[test]
fn test_spawner_creation() {
    let configs = vec![McpServerConfig {
        name: "test-server".to_string(),
        template: Some("custom".to_string()),
        description: Some("test".to_string()),
        command: "echo".to_string(),
        args: vec!["hello".to_string()],
        profile: SandboxProfile::default(),
        cached_tools: Vec::new(),
        enabled: true,
    }];

    let spawner = McpSpawner::new(configs);
    assert_eq!(spawner.configured_servers().len(), 1);
}

#[test]
fn test_disabled_server_not_loaded() {
    let configs = vec![McpServerConfig {
        name: "disabled-server".to_string(),
        template: Some("custom".to_string()),
        description: Some("disabled".to_string()),
        command: "echo".to_string(),
        args: vec![],
        profile: SandboxProfile::default(),
        cached_tools: Vec::new(),
        enabled: false,
    }];

    let spawner = McpSpawner::new(configs);
    assert_eq!(spawner.configured_servers().len(), 0);
}

#[tokio::test]
async fn test_unknown_server_rejected() {
    let spawner = McpSpawner::new(vec![]);
    let result = spawner.start_server("unknown").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_server_handle() {
    let configs = vec![McpServerConfig {
        name: "test".to_string(),
        template: Some("custom".to_string()),
        description: Some("test".to_string()),
        command: "echo".to_string(),
        args: vec![],
        profile: SandboxProfile::default(),
        cached_tools: Vec::new(),
        enabled: true,
    }];

    let spawner = Arc::new(McpSpawner::new(configs));
    let server = McpServer::new(spawner, "test".to_string());
    assert!(!server.is_running().await);
}
