use proptest::prelude::*;
use sdk::errors::{EngineError, RoveErrorExt};

// Task 2.3: Write property test for error user hints
// Property 31: Error User Hint Completeness
// Validates: Requirements 1.9, 20.4
proptest! {
    #[test]
    fn test_error_user_hint_completeness(error_str in "\\PC*") {
        // Encompass various error types to ensure they always return a valid user hint string
        // that doesn't panic or expose raw internal data directly in typical cases.
        let errs = vec![
            EngineError::Config(error_str.clone()),
            EngineError::Database(error_str.clone()),
            EngineError::LLMProvider(error_str.clone()),
            EngineError::ToolNotFound(error_str.clone()),
            EngineError::ToolError(error_str.clone()),
            EngineError::PathDenied(std::path::PathBuf::from(&error_str)),
            EngineError::HashMismatch(error_str.clone()),
        ];

        for err in errs {
            let hint = err.user_hint();
            // Hint should not be empty
            prop_assert!(!hint.is_empty());

            // Note: Since these are user-safe hints, they should generally be static strings
            // or well-formatted safe strings, avoiding the raw internal `error_str`
            // unless heavily scrubbed. Our implementation mostly uses static strings.
            prop_assert!(!hint.contains("core_tool.rs"));
        }
    }
}

// Task 2.6: Write property test for manifest parsing round-trip
// Property 29: Manifest Parsing Round-Trip
// Validates: Requirements 28.6
proptest! {
    #[test]
    fn test_manifest_roundtrip(
        version in "[0-9]+\\.[0-9]+\\.[0-9]+",
        team_key in "ed25519:[a-zA-Z0-9]{32}",
        signature in "ed25519:[a-zA-Z0-9]{64}",
        generated_at in "[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z",
        tool_name in "[a-z0-9-]+",
        plugin_name in "[a-z0-9-]+",
        path_allowed in "[a-z0-9/_-]+"
    ) {
        use sdk::manifest::{Manifest, CoreToolEntry, PluginEntry, PluginPermissions};

        // Construct a syntactically valid model from random inputs
        let manifest = Manifest {
            version: version.clone(),
            team_public_key: team_key.clone(),
            signature: signature.clone(),
            generated_at: generated_at.clone(),
            core_tools: vec![
                CoreToolEntry {
                    name: tool_name.clone(),
                    version: version.clone(),
                    path: format!("core-tools/{}.so", tool_name),
                    hash: "sha256:somehash".to_string(),
                    signature: "ed25519:somesig".to_string(),
                    platform: "linux-x86_64".to_string(),
                }
            ],
            plugins: vec![
                PluginEntry {
                    name: plugin_name.clone(),
                    version: version.clone(),
                    path: format!("plugins/{}.wasm", plugin_name),
                    hash: "sha256:somehash2".to_string(),
                    permissions: PluginPermissions {
                        allowed_paths: vec![path_allowed],
                        ..Default::default()
                    },
                    ..Default::default()
                }
            ]
        };

        let json = manifest.to_json().expect("Failed to serialize manifest");
        let parsed = Manifest::from_json(&json).expect("Failed to deserialize manifest");

        prop_assert_eq!(manifest.version, parsed.version);
        prop_assert_eq!(manifest.team_public_key, parsed.team_public_key);
        prop_assert_eq!(manifest.signature, parsed.signature);
        prop_assert_eq!(&manifest.core_tools[0].name, &parsed.core_tools[0].name);
        prop_assert_eq!(&manifest.plugins[0].name, &parsed.plugins[0].name);
        prop_assert_eq!(&manifest.plugins[0].permissions.allowed_paths, &parsed.plugins[0].permissions.allowed_paths);
    }
}
