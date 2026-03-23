use anyhow::{bail, Context, Result};

use crate::cli::SecretBackendArg;
use crate::config::metadata::SERVICE_NAME;
use crate::config::{Config, SecretBackend};
use crate::security::secrets::SecretManager;

const KNOWN_SECRETS: &[(&str, &str)] = &[
    ("openai", "openai_api_key"),
    ("anthropic", "anthropic_api_key"),
    ("gemini", "gemini_api_key"),
    ("nvidia_nim", "nvidia_nim_api_key"),
    ("telegram", "telegram_token"),
    ("zerotier", "zerotier_api_token"),
    ("webui", "webui_token"),
];

pub async fn set(name: &str) -> Result<()> {
    let key = resolve_secret_key(name)?;
    let prompt = format!("Enter value for {}: ", name);
    let value = rpassword::read_password_from_tty(Some(&prompt))
        .context("Failed to read secret from terminal")?;
    let value = value.trim();
    if value.is_empty() {
        bail!("Secret cannot be empty");
    }

    let manager = SecretManager::new(SERVICE_NAME);
    manager.set_secret(key, value).await?;

    println!("Stored secret '{}'", name);
    Ok(())
}

pub async fn list() -> Result<()> {
    let manager = SecretManager::new(SERVICE_NAME);

    println!("Configured secrets:");
    for (name, key) in KNOWN_SECRETS {
        let status = if manager.has_secret(key).await {
            "configured"
        } else {
            "missing"
        };
        println!("  {:<12} {}", name, status);
    }

    Ok(())
}

pub async fn remove(name: &str) -> Result<()> {
    let key = resolve_secret_key(name)?;
    let manager = SecretManager::new(SERVICE_NAME);
    manager.delete_secret(key).await?;
    println!("Removed secret '{}'", name);
    Ok(())
}

pub fn show_backend() -> Result<()> {
    let manager = SecretManager::new(SERVICE_NAME);
    println!("secret_backend: {}", manager.configured_backend().as_str());
    Ok(())
}

pub fn set_backend(backend: SecretBackendArg) -> Result<()> {
    let mut config = Config::load_or_create()?;
    config.secrets.backend = match backend {
        SecretBackendArg::Auto => SecretBackend::Auto,
        SecretBackendArg::Vault => SecretBackend::Vault,
        SecretBackendArg::Keychain => SecretBackend::Keychain,
        SecretBackendArg::Env => SecretBackend::Env,
    };
    config.save()?;
    println!("secret_backend: {}", config.secrets.backend.as_str());
    Ok(())
}

fn resolve_secret_key(name: &str) -> Result<&'static str> {
    let normalized = name.trim().to_ascii_lowercase();
    KNOWN_SECRETS
        .iter()
        .find_map(|(alias, key)| (*alias == normalized).then_some(*key))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Unknown secret '{}'. Supported secrets: {}",
                name,
                KNOWN_SECRETS
                    .iter()
                    .map(|(alias, _)| *alias)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}
