/// Example demonstrating SecretManager usage for storing and retrieving API keys
///
/// This example shows how to:
/// 1. Create a SecretManager instance
/// 2. Store secrets in the OS keychain
/// 3. Retrieve secrets from the keychain
/// 4. Handle missing secrets with interactive prompts
///
/// Run with: cargo run --example secret_manager_usage
use rove_engine::secrets::SecretManager;

#[tokio::main]
async fn main() {
    // Initialize tracing for logging
    tracing_subscriber::fmt::init();

    println!("=== SecretManager Usage Example ===\n");

    // Create a SecretManager instance
    let manager = SecretManager::new("rove-example");

    // Example 1: Store a secret
    println!("1. Storing an API key in the keychain...");
    let api_key = "sk-example-api-key-12345";
    match manager.set_secret("example_api_key", api_key).await {
        Ok(_) => println!("   ✓ API key stored successfully\n"),
        Err(e) => {
            eprintln!("   ✗ Failed to store API key: {}", e);
            return;
        }
    }

    // Example 2: Retrieve the secret
    println!("2. Retrieving the API key from the keychain...");
    match manager.get_secret("example_api_key").await {
        Ok(retrieved_key) => {
            println!("   ✓ Retrieved API key: {}", mask_secret(&retrieved_key));
            assert_eq!(
                retrieved_key, api_key,
                "Retrieved key should match stored key"
            );
            println!("   ✓ Verification passed\n");
        }
        Err(e) => {
            eprintln!("   ✗ Failed to retrieve API key: {}", e);
            return;
        }
    }

    // Example 3: Demonstrate interactive prompting (commented out to avoid blocking)
    println!("3. Interactive prompting for missing secrets:");
    println!("   When a secret is not found, get_secret() will prompt the user.");
    println!("   Example: manager.get_secret(\"new_key\") would prompt:");
    println!("   'Enter value for 'new_key': '\n");

    // Example 4: Update a secret
    println!("4. Updating the API key...");
    let new_api_key = "sk-updated-api-key-67890";
    match manager.set_secret("example_api_key", new_api_key).await {
        Ok(_) => println!("   ✓ API key updated successfully\n"),
        Err(e) => {
            eprintln!("   ✗ Failed to update API key: {}", e);
            return;
        }
    }

    // Verify the update
    match manager.get_secret("example_api_key").await {
        Ok(retrieved_key) => {
            println!(
                "   ✓ Retrieved updated key: {}",
                mask_secret(&retrieved_key)
            );
            assert_eq!(
                retrieved_key, new_api_key,
                "Retrieved key should match updated key"
            );
            println!("   ✓ Update verification passed\n");
        }
        Err(e) => {
            eprintln!("   ✗ Failed to retrieve updated API key: {}", e);
            return;
        }
    }

    // Example 5: Clean up
    println!("5. Cleaning up (deleting the secret)...");
    match manager.delete_secret("example_api_key").await {
        Ok(_) => println!("   ✓ API key deleted successfully\n"),
        Err(e) => {
            eprintln!("   ✗ Failed to delete API key: {}", e);
            return;
        }
    }

    println!("=== Example completed successfully ===");
}

/// Masks a secret for display purposes, showing only the first 8 characters
fn mask_secret(secret: &str) -> String {
    if secret.len() <= 8 {
        "*".repeat(secret.len())
    } else {
        format!("{}...", &secret[..8])
    }
}
