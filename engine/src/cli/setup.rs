use std::path::PathBuf;

use anyhow::Result;

use crate::cli::database_path::database_path;
use crate::config::{metadata::SERVICE_NAME, Config, CustomProvider};
use crate::security::secrets::SecretManager;
use crate::storage::Database;

use super::tui::setup;

pub async fn handle_setup() -> Result<()> {
    let result = setup::run_setup_wizard()?;

    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    let config_dir = home.join(".rove");
    std::fs::create_dir_all(&config_dir)?;

    if !result.api_key.is_empty() && !result.secret_key.is_empty() {
        let secret_manager = SecretManager::new(SERVICE_NAME);
        secret_manager.set_secret(&result.secret_key, &result.api_key).await?;
    }

    let mut config = Config::default();
    config.core.workspace = PathBuf::from(&result.workspace);
    config.security.max_risk_tier = result.max_risk_tier;

    if result.skipped_model {
        config.llm.default_provider = "ollama".to_string();
    } else {
        apply_provider(&mut config, &result);
    }

    let config_path = config_dir.join("config.toml");
    let config_content = toml::to_string_pretty(&config)?;
    std::fs::write(&config_path, config_content)?;

    let db_path = database_path(&config);
    let db_exists = db_path.exists();
    if !db_exists {
        let database = Database::new(&db_path).await?;
        drop(database);
    }

    let workspace_path = expand_workspace(&result.workspace, &home);
    std::fs::create_dir_all(&workspace_path)?;

    setup::print_summary(&result, &config_path.display().to_string(), true);
    Ok(())
}

fn apply_provider(config: &mut Config, result: &setup::SetupResult) {
    config.llm.default_provider = result.provider_name.clone();

    match result.provider_name.as_str() {
        "ollama" => {
            config.llm.ollama.base_url = result.base_url.clone();
            config.llm.ollama.model = result.model.clone();
        }
        "openai" => {
            config.llm.openai.base_url = result.base_url.clone();
            config.llm.openai.model = result.model.clone();
        }
        "anthropic" => {
            config.llm.anthropic.base_url = result.base_url.clone();
            config.llm.anthropic.model = result.model.clone();
        }
        "gemini" => {
            config.llm.gemini.base_url = result.base_url.clone();
            config.llm.gemini.model = result.model.clone();
        }
        _ => {
            config.llm.custom_providers.retain(|provider| provider.name != result.provider_name);
            config.llm.custom_providers.push(CustomProvider {
                name: result.provider_name.clone(),
                protocol: result.protocol.clone(),
                base_url: result.base_url.clone(),
                model: result.model.clone(),
                secret_key: result.secret_key.clone(),
            });
        }
    }
}

fn expand_workspace(workspace: &str, home: &std::path::Path) -> PathBuf {
    if let Some(rest) = workspace.strip_prefix("~/") {
        return home.join(rest);
    }

    PathBuf::from(workspace)
}
