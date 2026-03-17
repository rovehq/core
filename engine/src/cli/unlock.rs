use std::sync::Arc;

use anyhow::Result;

use crate::config::metadata::SERVICE_NAME;
use crate::security::secrets::{SecretCache, SecretManager};

pub async fn run() -> Result<()> {
    println!();
    println!("  Unlocking secrets...");
    println!("     Priority: environment variables > OS keychain");
    println!("     (Keychain may prompt once)");
    println!();

    let secret_manager = Arc::new(SecretManager::new(SERVICE_NAME));
    let secret_cache = Arc::new(SecretCache::new(secret_manager.clone()));
    let api_keys = vec![
        "openai_api_key",
        "anthropic_api_key",
        "gemini_api_key",
        "nvidia_nim_api_key",
    ];

    match secret_cache.unlock(&api_keys).await {
        Ok(()) => {
            println!("  Secrets unlocked successfully.");
            println!("  Keys remain in memory until the daemon stops.");
        }
        Err(error) => {
            println!("  Partial unlock: {}", error);
        }
    }

    println!();
    println!("  Loaded keys:");
    for key in &api_keys {
        if secret_manager.has_secret(key).await {
            let env_key = format!(
                "{}_{}",
                SERVICE_NAME.to_uppercase().replace('-', "_"),
                key.to_uppercase()
            );
            let source =
                if std::env::var(&env_key).is_ok() || std::env::var(key.to_uppercase()).is_ok() {
                    "(env)"
                } else {
                    "(keychain)"
                };
            println!("    {} {}", key, source);
        } else {
            println!("    {} (not configured)", key);
        }
    }

    println!();
    println!("  Tip: set environment variables to skip keychain prompts.");
    println!("    export ROVE_OPENAI_API_KEY=sk-...");
    println!("    export ROVE_ANTHROPIC_API_KEY=sk-ant-...");
    println!();
    Ok(())
}
