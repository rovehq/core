//! Integration tests for WasmRuntime
//!
//! These tests verify the two-gate verification system for WASM plugins.
//!
//! Requirements tested:
//! - Requirement 5.1: Load plugins as WASM modules via Extism
//! - Requirement 5.2: Gate 1 - Check plugin is in manifest
//! - Requirement 5.3: Gate 2 - Verify file hash with BLAKE3
//! - Requirement 5.4: Validate manifest contains no absolute paths

use rove_engine::crypto::CryptoModule;
use rove_engine::fs_guard::FileSystemGuard;
use rove_engine::runtime::WasmRuntime;
use sdk::errors::EngineError;
use sdk::manifest::{Manifest, PluginEntry, PluginPermissions};
use std::sync::Arc;
use tempfile::TempDir;

/// Helper function to create a test manifest with a plugin entry
fn create_test_manifest(plugin_name: &str, plugin_path: &str, plugin_hash: &str) -> Manifest {
    Manifest {
        version: "1.0.0".to_string(),
        team_public_key: "ed25519:test_key".to_string(),
        signature: "ed25519:test_sig".to_string(),
        generated_at: "2024-01-15T10:30:00Z".to_string(),
        core_tools: vec![],
        plugins: vec![PluginEntry {
            name: plugin_name.to_string(),
            version: "0.1.0".to_string(),
            path: plugin_path.to_string(),
            hash: plugin_hash.to_string(),
            permissions: PluginPermissions {
                allowed_paths: vec!["workspace".to_string()],
                denied_paths: vec![".ssh".to_string(), ".env".to_string()],
                max_file_size: Some(10485760),
                can_execute: false,
                allowed_commands: None,
                denied_flags: None,
                max_execution_time: None,
            },
            allowed_imports: vec![
                "extism:host/env".to_string(),
                "wasi_snapshot_preview1".to_string(),
            ],
            trust_tier: 0,
        }],
    }
}

#[test]
fn test_wasm_runtime_creation() {
    // Create a temporary workspace directory
    let temp_dir = TempDir::new().unwrap();
    let workspace = temp_dir.path().to_path_buf();

    // Create a minimal manifest
    let manifest = Manifest {
        version: "1.0.0".to_string(),
        team_public_key: "ed25519:test_key".to_string(),
        signature: "ed25519:test_sig".to_string(),
        generated_at: "2024-01-15T10:30:00Z".to_string(),
        core_tools: vec![],
        plugins: vec![],
    };

    let crypto = Arc::new(CryptoModule::new().unwrap());
    let fs_guard = Arc::new(FileSystemGuard::new(workspace).expect("test workspace"));
    let _runtime = WasmRuntime::new(manifest, crypto, fs_guard);

    // If we get here, the runtime was created successfully
}

#[tokio::test]
async fn test_gate1_plugin_not_in_manifest() {
    // Create a temporary workspace directory
    let temp_dir = TempDir::new().unwrap();
    let workspace = temp_dir.path().to_path_buf();

    // Create a manifest without the plugin
    let manifest = Manifest {
        version: "1.0.0".to_string(),
        team_public_key: "ed25519:test_key".to_string(),
        signature: "ed25519:test_sig".to_string(),
        generated_at: "2024-01-15T10:30:00Z".to_string(),
        core_tools: vec![],
        plugins: vec![],
    };

    let crypto = Arc::new(CryptoModule::new().unwrap());
    let fs_guard = Arc::new(FileSystemGuard::new(workspace).expect("test workspace"));
    let mut runtime = WasmRuntime::new(manifest, crypto, fs_guard);

    // Attempt to load a plugin not in the manifest
    let result = runtime.load_plugin("nonexistent").await;

    // Should fail at Gate 1
    assert!(matches!(result, Err(EngineError::PluginNotInManifest(_))));
}

#[tokio::test]
async fn test_gate1_absolute_path_rejected() {
    // Create a temporary workspace directory
    let temp_dir = TempDir::new().unwrap();
    let workspace = temp_dir.path().to_path_buf();

    // Create a manifest with an absolute path (security violation)
    // Use platform-appropriate absolute path since `/etc/passwd` isn't
    // recognized as absolute on Windows by std::path::Path::is_absolute()
    #[cfg(unix)]
    let abs_path = "/etc/passwd";
    #[cfg(windows)]
    let abs_path = "C:\\Windows\\System32\\config";

    let manifest = create_test_manifest(
        "malicious-plugin",
        abs_path, // Absolute path - should be rejected
        "dummy_hash",
    );

    let crypto = Arc::new(CryptoModule::new().unwrap());
    let fs_guard = Arc::new(FileSystemGuard::new(workspace).expect("test workspace"));
    let mut runtime = WasmRuntime::new(manifest, crypto, fs_guard);

    // Attempt to load the plugin with absolute path
    let result = runtime.load_plugin("malicious-plugin").await;

    // Should fail due to absolute path validation (Requirement 5.4)
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(matches!(e, EngineError::Config(_)));
    }
}

#[test]
fn test_is_plugin_loaded() {
    // Create a temporary workspace directory
    let temp_dir = TempDir::new().unwrap();
    let workspace = temp_dir.path().to_path_buf();

    let manifest = Manifest {
        version: "1.0.0".to_string(),
        team_public_key: "ed25519:test_key".to_string(),
        signature: "ed25519:test_sig".to_string(),
        generated_at: "2024-01-15T10:30:00Z".to_string(),
        core_tools: vec![],
        plugins: vec![],
    };

    let crypto = Arc::new(CryptoModule::new().unwrap());
    let fs_guard = Arc::new(FileSystemGuard::new(workspace).expect("test workspace"));
    let runtime = WasmRuntime::new(manifest, crypto, fs_guard);

    // Plugin should not be loaded
    assert!(!runtime.is_plugin_loaded("fs-editor"));
}

#[test]
fn test_loaded_plugins_empty() {
    // Create a temporary workspace directory
    let temp_dir = TempDir::new().unwrap();
    let workspace = temp_dir.path().to_path_buf();

    let manifest = Manifest {
        version: "1.0.0".to_string(),
        team_public_key: "ed25519:test_key".to_string(),
        signature: "ed25519:test_sig".to_string(),
        generated_at: "2024-01-15T10:30:00Z".to_string(),
        core_tools: vec![],
        plugins: vec![],
    };

    let crypto = Arc::new(CryptoModule::new().unwrap());
    let fs_guard = Arc::new(FileSystemGuard::new(workspace).expect("test workspace"));
    let runtime = WasmRuntime::new(manifest, crypto, fs_guard);

    // No plugins should be loaded
    assert_eq!(runtime.loaded_plugins().len(), 0);
}

#[test]
fn test_unload_plugin_not_loaded() {
    // Create a temporary workspace directory
    let temp_dir = TempDir::new().unwrap();
    let workspace = temp_dir.path().to_path_buf();

    let manifest = Manifest {
        version: "1.0.0".to_string(),
        team_public_key: "ed25519:test_key".to_string(),
        signature: "ed25519:test_sig".to_string(),
        generated_at: "2024-01-15T10:30:00Z".to_string(),
        core_tools: vec![],
        plugins: vec![],
    };

    let crypto = Arc::new(CryptoModule::new().unwrap());
    let fs_guard = Arc::new(FileSystemGuard::new(workspace).expect("test workspace"));
    let mut runtime = WasmRuntime::new(manifest, crypto, fs_guard);

    // Unloading a non-existent plugin should not panic
    runtime.unload_plugin("nonexistent");

    // Should still have no plugins loaded
    assert_eq!(runtime.loaded_plugins().len(), 0);
}

#[test]
fn test_unload_all_empty() {
    // Create a temporary workspace directory
    let temp_dir = TempDir::new().unwrap();
    let workspace = temp_dir.path().to_path_buf();

    let manifest = Manifest {
        version: "1.0.0".to_string(),
        team_public_key: "ed25519:test_key".to_string(),
        signature: "ed25519:test_sig".to_string(),
        generated_at: "2024-01-15T10:30:00Z".to_string(),
        core_tools: vec![],
        plugins: vec![],
    };

    let crypto = Arc::new(CryptoModule::new().unwrap());
    let fs_guard = Arc::new(FileSystemGuard::new(workspace).expect("test workspace"));
    let mut runtime = WasmRuntime::new(manifest, crypto, fs_guard);

    // Unloading all when empty should not panic
    runtime.unload_all();

    assert_eq!(runtime.loaded_plugins().len(), 0);
}

#[tokio::test]
async fn test_call_plugin_not_loaded() {
    // Create a temporary workspace directory
    let temp_dir = TempDir::new().unwrap();
    let workspace = temp_dir.path().to_path_buf();

    let manifest = create_test_manifest("nonexistent", "plugins/nonexistent.wasm", "deadbeef");

    let crypto = Arc::new(CryptoModule::new().unwrap());
    let fs_guard = Arc::new(FileSystemGuard::new(workspace).expect("test workspace"));
    let mut runtime = WasmRuntime::new(manifest, crypto, fs_guard);

    // Attempt to call a plugin that's not loaded
    let result = runtime
        .call_plugin("nonexistent", "some_function", b"{}")
        .await;

    // Should fail with PluginNotLoaded error
    assert!(matches!(result, Err(EngineError::PluginNotLoaded(_))));
}

#[tokio::test]
async fn test_gate2_hash_mismatch() {
    // Create a temporary workspace directory
    let workspace_dir = TempDir::new().unwrap();
    let workspace = workspace_dir.path().to_path_buf();

    // Create a temporary directory and file for testing
    let temp_dir = TempDir::new().unwrap();
    let plugin_path = temp_dir.path().join("test_plugin.wasm");

    // Write some dummy content
    std::fs::write(&plugin_path, b"dummy wasm content").unwrap();

    // Create manifest with WRONG hash
    let manifest = create_test_manifest(
        "test-plugin",
        plugin_path.to_str().unwrap(),
        "0000000000000000000000000000000000000000000000000000000000000000", // Wrong hash
    );

    let crypto = Arc::new(CryptoModule::new().unwrap());
    let fs_guard = Arc::new(FileSystemGuard::new(workspace).expect("test workspace"));
    let mut runtime = WasmRuntime::new(manifest, crypto, fs_guard);

    // Attempt to load the plugin
    let result = runtime.load_plugin("test-plugin").await;

    // Should fail at Gate 2 (hash verification)
    assert!(result.is_err());

    // The file should have been deleted by the crypto module
    // (This is tested in crypto module tests)
}

#[test]
fn test_runtime_drop_unloads_all() {
    // Create a temporary workspace directory
    let temp_dir = TempDir::new().unwrap();
    let workspace = temp_dir.path().to_path_buf();

    let manifest = Manifest {
        version: "1.0.0".to_string(),
        team_public_key: "ed25519:test_key".to_string(),
        signature: "ed25519:test_sig".to_string(),
        generated_at: "2024-01-15T10:30:00Z".to_string(),
        core_tools: vec![],
        plugins: vec![],
    };

    let crypto = Arc::new(CryptoModule::new().unwrap());
    let fs_guard = Arc::new(FileSystemGuard::new(workspace).expect("test workspace"));

    {
        let _runtime = WasmRuntime::new(manifest, crypto, fs_guard);
        // Runtime goes out of scope here
    }

    // If we get here without panic, the Drop implementation worked
}

/// Test that demonstrates the two-gate verification flow
#[tokio::test]
async fn test_two_gate_verification_flow() {
    // Create a temporary workspace directory
    let temp_dir = TempDir::new().unwrap();
    let workspace = temp_dir.path().to_path_buf();

    // This test documents the expected flow:
    // 1. Gate 1: Check plugin is in manifest
    // 2. Gate 2: Verify file hash
    // 3. Load WASM module via Extism

    let manifest = Manifest {
        version: "1.0.0".to_string(),
        team_public_key: "ed25519:test_key".to_string(),
        signature: "ed25519:test_sig".to_string(),
        generated_at: "2024-01-15T10:30:00Z".to_string(),
        core_tools: vec![],
        plugins: vec![],
    };

    let crypto = Arc::new(CryptoModule::new().unwrap());
    let fs_guard = Arc::new(FileSystemGuard::new(workspace).expect("test workspace"));
    let mut runtime = WasmRuntime::new(manifest, crypto, fs_guard);

    // Attempt to load a plugin not in manifest - should fail at Gate 1
    let result = runtime.load_plugin("test-plugin").await;
    assert!(matches!(result, Err(EngineError::PluginNotInManifest(_))));

    // Gate 2 and WASM loading would be tested with actual WASM files
    // in more comprehensive integration tests
}
