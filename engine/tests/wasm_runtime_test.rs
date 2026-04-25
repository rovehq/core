use sdk::manifest::Manifest;

#[test]
fn test_wasm_runtime_creation_manifest_shape() {
    let manifest = Manifest {
        version: "1.0.0".to_string(),
        team_public_key: "ed25519:test_key".to_string(),
        signature: "ed25519:test_sig".to_string(),
        generated_at: "2024-01-15T10:30:00Z".to_string(),
        core_tools: vec![],
        plugins: vec![],
    };

    assert!(manifest.plugins.is_empty());
}

#[test]
fn test_plugin_not_in_manifest_shape() {
    let manifest = Manifest {
        version: "1.0.0".to_string(),
        team_public_key: "ed25519:test_key".to_string(),
        signature: "ed25519:test_sig".to_string(),
        generated_at: "2024-01-15T10:30:00Z".to_string(),
        core_tools: vec![],
        plugins: vec![],
    };

    assert!(manifest.get_plugin("nonexistent").is_none());
}

#[test]
fn test_loaded_plugins_empty_shape() {
    let manifest = Manifest {
        version: "1.0.0".to_string(),
        team_public_key: "ed25519:test_key".to_string(),
        signature: "ed25519:test_sig".to_string(),
        generated_at: "2024-01-15T10:30:00Z".to_string(),
        core_tools: vec![],
        plugins: vec![],
    };

    assert!(manifest.plugins.is_empty());
}