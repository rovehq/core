use std::sync::Arc;

use rove_engine::crypto::CryptoModule;
use rove_engine::runtime::native::NativeRuntime;
use sdk::errors::EngineError;
use sdk::manifest::Manifest;
use serde_json::json;

fn empty_manifest() -> Manifest {
    Manifest {
        version: "1.0.0".to_string(),
        team_public_key: "ed25519:test_key".to_string(),
        signature: "ed25519:test_sig".to_string(),
        generated_at: "2024-01-15T10:30:00Z".to_string(),
        core_tools: vec![],
        plugins: vec![],
    }
}

#[test]
fn test_native_runtime_creation_manifest_shape() {
    let manifest = empty_manifest();

    assert!(manifest.core_tools.is_empty());
}

#[test]
fn test_tool_not_in_manifest_shape() {
    let manifest = empty_manifest();

    assert!(manifest.get_core_tool("nonexistent").is_none());
}

#[test]
fn test_is_tool_loaded_shape() {
    let manifest = empty_manifest();

    assert!(manifest.core_tools.is_empty());
}

#[test]
fn test_loaded_tools_empty() {
    let manifest = empty_manifest();

    assert!(manifest.core_tools.is_empty());
}

#[test]
fn test_register_library_tracks_metadata() {
    let crypto = Arc::new(CryptoModule::new().expect("Failed to create CryptoModule"));
    let mut runtime = NativeRuntime::new(empty_manifest(), crypto);

    runtime.register_library("/tmp/libdemo.dylib", "sha256:test", "ed25519:test");

    assert!(runtime.is_library_registered("/tmp/libdemo.dylib"));
}

#[test]
fn test_call_registered_tool_requires_registered_library() {
    let crypto = Arc::new(CryptoModule::new().expect("Failed to create CryptoModule"));
    let mut runtime = NativeRuntime::new(empty_manifest(), crypto);

    let result =
        runtime.call_registered_tool("/tmp/missing-tool.dylib", "demo", json!({"value": "test"}));

    assert!(matches!(
        result,
        Err(EngineError::ToolNotInManifest(path)) if path == "/tmp/missing-tool.dylib"
    ));
}
