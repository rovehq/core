use rove_engine::secrets::SecretManager;

#[tokio::test]
async fn test_secret_manager_integration() {
    if std::env::var("CI").is_ok() {
        return; // Skip: no keyring in CI
    }
    let manager = SecretManager::new("rove-integration-test");

    // Test storing and retrieving a secret
    let key = "test_api_key_integration";
    let value = "sk-test123456789";

    // Store the secret
    manager
        .set_secret(key, value)
        .await
        .expect("Failed to store secret");

    // Retrieve the secret
    let retrieved = manager.get_secret(key).await.expect("Failed to retrieve secret");

    assert_eq!(
        retrieved, value,
        "Retrieved secret should match stored value"
    );

    // Clean up
    manager.delete_secret(key).await.expect("Failed to delete secret");
}

#[tokio::test]
async fn test_secret_manager_multiple_keys() {
    if std::env::var("CI").is_ok() {
        return; // Skip: no keyring in CI
    }
    let manager = SecretManager::new("rove-integration-test");

    let keys_and_values = vec![
        ("openai_key", "sk-openai123"),
        ("anthropic_key", "sk-ant-456"),
        ("gemini_key", "AIza789"),
    ];

    // Store all secrets
    for (key, value) in &keys_and_values {
        manager
            .set_secret(key, value)
            .await
            .unwrap_or_else(|_| panic!("Failed to store {}", key));
    }

    // Retrieve and verify all secrets
    for (key, expected_value) in &keys_and_values {
        let retrieved = manager
            .get_secret(key)
            .await
            .unwrap_or_else(|_| panic!("Failed to retrieve {}", key));
        assert_eq!(&retrieved, expected_value, "Secret {} should match", key);
    }

    // Clean up all secrets
    for (key, _) in &keys_and_values {
        manager
            .delete_secret(key)
            .await
            .unwrap_or_else(|_| panic!("Failed to delete {}", key));
    }
}

#[tokio::test]
async fn test_secret_manager_overwrite() {
    if std::env::var("CI").is_ok() {
        return; // Skip: no keyring in CI
    }
    let manager = SecretManager::new("rove-integration-test");
    let key = "test_overwrite_key";

    // Store initial value
    manager
        .set_secret(key, "initial_value")
        .await
        .expect("Failed to store initial value");

    // Overwrite with new value
    manager
        .set_secret(key, "new_value")
        .await
        .expect("Failed to overwrite value");

    // Verify new value is retrieved
    let retrieved = manager.get_secret(key).await.expect("Failed to retrieve value");
    assert_eq!(
        retrieved, "new_value",
        "Should retrieve the overwritten value"
    );

    // Clean up
    manager.delete_secret(key).await.expect("Failed to delete secret");
}

#[test]
fn test_scrub_integration_with_real_patterns() {
    let manager = SecretManager::new("rove-integration-test");

    // Test with realistic API key patterns
    let test_cases = vec![
        (
            "Error: Authentication failed with key sk-proj-1234567890abcdefghijklmnopqrstuvwxyz",
            "Error: Authentication failed with key [REDACTED]"
        ),
        (
            "Using Google API key AIza12345678901234567890123456789012345 for geocoding",
            "Using Google API key [REDACTED] for geocoding"
        ),
        (
            "Telegram bot initialized with token 1234567890:ABCDEFGHIJKLMNOPQRSTUVWXYZ123456789",
            "Telegram bot initialized with token [REDACTED]"
        ),
        (
            "GitHub token ghp_1234567890abcdefghijklmnopqrstuvwxyz used for API",
            "GitHub token [REDACTED] used for API"
        ),
        (
            "Authorization header: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0",
            "Authorization header: [REDACTED]"
        ),
    ];

    for (input, expected) in test_cases {
        let scrubbed = manager.scrub(input);
        assert_eq!(scrubbed, expected, "Failed to scrub: {}", input);
    }
}

#[test]
fn test_scrub_integration_multiple_secrets_in_text() {
    let manager = SecretManager::new("rove-integration-test");

    let log_message = r#"
    [INFO] Initializing services
    [DEBUG] OpenAI API key: sk-proj-abcdefghijklmnopqrstuvwxyz123456
    [DEBUG] GitHub token: ghp_1234567890abcdefghijklmnopqrstuvwxyz
    [DEBUG] Telegram bot: 9876543210:ZYXWVUTSRQPONMLKJIHGFEDCBA987654321
    [INFO] All services initialized
    "#;

    let scrubbed = manager.scrub(log_message);

    // Verify all secrets are scrubbed
    assert!(!scrubbed.contains("sk-proj-abcdefghijklmnopqrstuvwxyz123456"));
    assert!(!scrubbed.contains("ghp_1234567890abcdefghijklmnopqrstuvwxyz"));
    assert!(!scrubbed.contains("9876543210:ZYXWVUTSRQPONMLKJIHGFEDCBA987654321"));

    // Verify [REDACTED] appears
    assert!(scrubbed.contains("[REDACTED]"));

    // Verify non-secret content is preserved
    assert!(scrubbed.contains("[INFO] Initializing services"));
    assert!(scrubbed.contains("[INFO] All services initialized"));
}

#[test]
fn test_scrub_integration_preserves_non_secrets() {
    let manager = SecretManager::new("rove-integration-test");

    let text = r#"
    Configuration loaded successfully.
    Workspace: /home/user/projects
    Log level: INFO
    Plugins enabled: fs-editor, terminal, git
    LLM providers: ollama, openai, anthropic
    "#;

    let scrubbed = manager.scrub(text);

    // Text should be unchanged since there are no secrets
    assert_eq!(scrubbed, text);
}

#[test]
fn test_scrub_integration_error_messages() {
    let manager = SecretManager::new("rove-integration-test");

    // Simulate error messages that might contain secrets
    let errors = vec![
        format!(
            "API request failed: Invalid key sk-{}",
            "1234567890abcdefghijklmnopqrstuvwxyz"
        ),
        format!(
            "Telegram bot error: Token {} is invalid",
            "1234567890:ABCDEFGHIJKLMNOPQRSTUVWXYZ123456789"
        ),
        format!(
            "GitHub API error: Token ghp_{} expired",
            "1234567890abcdefghijklmnopqrstuvwxyz"
        ),
    ];

    for error in errors {
        let scrubbed = manager.scrub(&error);

        // Verify the secret is scrubbed
        assert!(
            scrubbed.contains("[REDACTED]"),
            "Error message should have secret scrubbed: {}",
            error
        );

        // Verify the error context is preserved
        assert!(
            scrubbed.contains("error") || scrubbed.contains("Error") || scrubbed.contains("failed")
        );
    }
}
