//! Integration tests for NativeRuntime
//!
//! These tests verify the tool loading mechanism with four-gate verification.

use rove_engine::crypto::CryptoModule;
use rove_engine::runtime::NativeRuntime;
use sdk::{
    core_tool::CoreContext,
    manifest::{CoreToolEntry, Manifest},
    AgentHandle, AgentHandleImpl, BusHandle, BusHandleImpl, ConfigHandle, ConfigHandleImpl,
    CryptoHandle, CryptoHandleImpl, DbHandle, DbHandleImpl, EngineError, NetworkHandle,
    NetworkHandleImpl,
};
use serde_json::json;
use std::sync::Arc;

// Mock implementations for testing
struct MockAgentHandle;
impl AgentHandleImpl for MockAgentHandle {
    fn submit_task(&self, _task_input: String) -> Result<String, EngineError> {
        Ok("task-123".to_string())
    }

    fn get_task_status(&self, _task_id: &str) -> Result<String, EngineError> {
        Ok("completed".to_string())
    }
}

struct MockDbHandle;
impl DbHandleImpl for MockDbHandle {
    fn query(
        &self,
        _sql: &str,
        _params: Vec<serde_json::Value>,
    ) -> Result<Vec<serde_json::Value>, EngineError> {
        Ok(vec![])
    }
}

struct MockConfigHandle;
impl ConfigHandleImpl for MockConfigHandle {
    fn get(&self, key: &str) -> Option<serde_json::Value> {
        match key {
            "core.workspace" => Some(json!("~/projects")),
            _ => None,
        }
    }
}

struct MockCryptoHandle;
impl CryptoHandleImpl for MockCryptoHandle {
    fn sign_data(&self, _data: &[u8]) -> Result<Vec<u8>, EngineError> {
        Ok(vec![0xDE, 0xAD, 0xBE, 0xEF])
    }

    fn verify_signature(&self, _data: &[u8], _signature: &[u8]) -> Result<(), EngineError> {
        Ok(())
    }

    fn get_secret(&self, _key: &str) -> Result<String, EngineError> {
        Ok("mock-secret".to_string())
    }

    fn scrub_secrets(&self, text: &str) -> String {
        text.to_string()
    }
}

struct MockNetworkHandle;
impl NetworkHandleImpl for MockNetworkHandle {
    fn http_get(&self, _url: &str) -> Result<Vec<u8>, EngineError> {
        Ok(b"Mock response".to_vec())
    }

    fn http_post(&self, _url: &str, _body: Vec<u8>) -> Result<Vec<u8>, EngineError> {
        Ok(b"Mock response".to_vec())
    }
}

struct MockBusHandle;
impl BusHandleImpl for MockBusHandle {
    fn subscribe(&self, _event_type: &str) -> Result<(), EngineError> {
        Ok(())
    }

    fn publish(&self, _event_type: &str, _payload: serde_json::Value) -> Result<(), EngineError> {
        Ok(())
    }
}

fn create_mock_context() -> CoreContext {
    let agent = AgentHandle::new(Arc::new(MockAgentHandle));
    let db = DbHandle::new(Arc::new(MockDbHandle));
    let config = ConfigHandle::new(Arc::new(MockConfigHandle));
    let crypto = CryptoHandle::new(Arc::new(MockCryptoHandle));
    let network = NetworkHandle::new(Arc::new(MockNetworkHandle));
    let bus = BusHandle::new(Arc::new(MockBusHandle));

    CoreContext::new(agent, db, config, crypto, network, bus)
}

#[test]
fn test_native_runtime_creation() {
    // Create a minimal manifest
    let manifest = Manifest {
        version: "1.0.0".to_string(),
        team_public_key: "ed25519:test_key".to_string(),
        signature: "ed25519:test_sig".to_string(),
        generated_at: "2024-01-15T10:30:00Z".to_string(),
        core_tools: vec![],
        plugins: vec![],
    };

    // Create crypto module (will use placeholder key in dev)
    let crypto = Arc::new(CryptoModule::new().expect("Failed to create CryptoModule"));

    // Create runtime
    let runtime = NativeRuntime::new(manifest, crypto);

    // Verify no tools are loaded initially
    assert_eq!(runtime.loaded_tools().len(), 0);
    assert!(!runtime.is_tool_loaded("nonexistent"));
}

#[test]
fn test_tool_not_in_manifest() {
    // Create a manifest without any tools
    let manifest = Manifest {
        version: "1.0.0".to_string(),
        team_public_key: "ed25519:test_key".to_string(),
        signature: "ed25519:test_sig".to_string(),
        generated_at: "2024-01-15T10:30:00Z".to_string(),
        core_tools: vec![],
        plugins: vec![],
    };

    let crypto = Arc::new(CryptoModule::new().expect("Failed to create CryptoModule"));
    let mut runtime = NativeRuntime::new(manifest, crypto);

    let ctx = create_mock_context();

    // Attempt to load a tool not in the manifest
    let result = runtime.load_tool("nonexistent", ctx);

    // Should fail at Gate 1
    assert!(result.is_err());
    match result {
        Err(EngineError::ToolNotInManifest(name)) => {
            assert_eq!(name, "nonexistent");
        }
        _ => panic!("Expected ToolNotInManifest error"),
    }
}

#[test]
fn test_is_tool_loaded() {
    let manifest = Manifest {
        version: "1.0.0".to_string(),
        team_public_key: "ed25519:test_key".to_string(),
        signature: "ed25519:test_sig".to_string(),
        generated_at: "2024-01-15T10:30:00Z".to_string(),
        core_tools: vec![],
        plugins: vec![],
    };

    let crypto = Arc::new(CryptoModule::new().expect("Failed to create CryptoModule"));
    let runtime = NativeRuntime::new(manifest, crypto);

    // Verify tool is not loaded
    assert!(!runtime.is_tool_loaded("telegram"));
    assert!(!runtime.is_tool_loaded("ui-server"));
}

#[test]
fn test_loaded_tools_empty() {
    let manifest = Manifest {
        version: "1.0.0".to_string(),
        team_public_key: "ed25519:test_key".to_string(),
        signature: "ed25519:test_sig".to_string(),
        generated_at: "2024-01-15T10:30:00Z".to_string(),
        core_tools: vec![],
        plugins: vec![],
    };

    let crypto = Arc::new(CryptoModule::new().expect("Failed to create CryptoModule"));
    let runtime = NativeRuntime::new(manifest, crypto);

    let loaded = runtime.loaded_tools();
    assert_eq!(loaded.len(), 0);
}

#[test]
fn test_unload_nonexistent_tool() {
    let manifest = Manifest {
        version: "1.0.0".to_string(),
        team_public_key: "ed25519:test_key".to_string(),
        signature: "ed25519:test_sig".to_string(),
        generated_at: "2024-01-15T10:30:00Z".to_string(),
        core_tools: vec![],
        plugins: vec![],
    };

    let crypto = Arc::new(CryptoModule::new().expect("Failed to create CryptoModule"));
    let mut runtime = NativeRuntime::new(manifest, crypto);

    // Unloading a non-existent tool should succeed silently
    let result = runtime.unload_tool("nonexistent");
    assert!(result.is_ok());
}

#[test]
fn test_call_tool_not_loaded() {
    let manifest = Manifest {
        version: "1.0.0".to_string(),
        team_public_key: "ed25519:test_key".to_string(),
        signature: "ed25519:test_sig".to_string(),
        generated_at: "2024-01-15T10:30:00Z".to_string(),
        core_tools: vec![],
        plugins: vec![],
    };

    let crypto = Arc::new(CryptoModule::new().expect("Failed to create CryptoModule"));
    let runtime = NativeRuntime::new(manifest, crypto);

    let input = sdk::types::ToolInput::new("test_method").with_param("key", json!("value"));

    // Calling a tool that's not loaded should fail
    let result = runtime.call_tool("nonexistent", input);
    assert!(result.is_err());
    match result {
        Err(EngineError::ToolNotLoaded(name)) => {
            assert_eq!(name, "nonexistent");
        }
        _ => panic!("Expected ToolNotLoaded error"),
    }
}

#[test]
fn test_unload_all_empty() {
    let manifest = Manifest {
        version: "1.0.0".to_string(),
        team_public_key: "ed25519:test_key".to_string(),
        signature: "ed25519:test_sig".to_string(),
        generated_at: "2024-01-15T10:30:00Z".to_string(),
        core_tools: vec![],
        plugins: vec![],
    };

    let crypto = Arc::new(CryptoModule::new().expect("Failed to create CryptoModule"));
    let mut runtime = NativeRuntime::new(manifest, crypto);

    // Unloading all when no tools are loaded should succeed
    runtime.unload_all();
    assert_eq!(runtime.loaded_tools().len(), 0);
}

#[test]
fn test_manifest_with_tool_entry() {
    // Create a manifest with a tool entry (but no actual file)
    let manifest = Manifest {
        version: "1.0.0".to_string(),
        team_public_key: "ed25519:test_key".to_string(),
        signature: "ed25519:test_sig".to_string(),
        generated_at: "2024-01-15T10:30:00Z".to_string(),
        core_tools: vec![CoreToolEntry {
            name: "test-tool".to_string(),
            version: "0.1.0".to_string(),
            path: "/nonexistent/path/libtest.so".to_string(),
            hash: "sha256:fakehash".to_string(),
            signature: "ed25519:fakesig".to_string(),
            platform: "linux-x86_64".to_string(),
        }],
        plugins: vec![],
    };

    let crypto = Arc::new(CryptoModule::new().expect("Failed to create CryptoModule"));
    let mut runtime = NativeRuntime::new(manifest, crypto);

    let ctx = create_mock_context();

    // Attempt to load the tool - should pass Gate 1 but fail at Gate 2 (hash verification)
    // because the file doesn't exist
    let result = runtime.load_tool("test-tool", ctx);

    // Should fail (either at hash verification or file not found)
    assert!(result.is_err());
}

#[test]
fn test_drop_calls_unload_all() {
    // Create a manifest
    let manifest = Manifest {
        version: "1.0.0".to_string(),
        team_public_key: "ed25519:test_key".to_string(),
        signature: "ed25519:test_sig".to_string(),
        generated_at: "2024-01-15T10:30:00Z".to_string(),
        core_tools: vec![],
        plugins: vec![],
    };

    let crypto = Arc::new(CryptoModule::new().expect("Failed to create CryptoModule"));

    // Create runtime in a scope so it gets dropped
    {
        let _runtime = NativeRuntime::new(manifest, crypto);
        // Runtime will be dropped here, calling unload_all()
    }

    // If we get here without panicking, the Drop implementation worked
    // This test verifies that Drop doesn't panic even with no tools loaded
}

#[test]
fn test_lifecycle_methods_exist() {
    // This test verifies that all lifecycle methods are implemented
    let manifest = Manifest {
        version: "1.0.0".to_string(),
        team_public_key: "ed25519:test_key".to_string(),
        signature: "ed25519:test_sig".to_string(),
        generated_at: "2024-01-15T10:30:00Z".to_string(),
        core_tools: vec![],
        plugins: vec![],
    };

    let crypto = Arc::new(CryptoModule::new().expect("Failed to create CryptoModule"));
    let mut runtime = NativeRuntime::new(manifest, crypto);

    // Test unload_tool (should succeed silently for non-existent tool)
    assert!(runtime.unload_tool("nonexistent").is_ok());

    // Test unload_all (should succeed with no tools)
    runtime.unload_all();

    // Verify no tools are loaded
    assert_eq!(runtime.loaded_tools().len(), 0);
}
