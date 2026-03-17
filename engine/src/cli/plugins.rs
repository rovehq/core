use anyhow::{Context, Result};
use serde_json::json;

use crate::cli::database_path::database_path;
use crate::config::Config;
use crate::storage::Database;

use super::output::OutputFormat;

pub async fn handle_list(config: &Config, format: OutputFormat) -> Result<()> {
    let _database = Database::new(&database_path(config))
        .await
        .context("Failed to open database")?;

    match format {
        OutputFormat::Text => {
            println!("Installed Plugins:");
            println!();
            println!(
                "  fs-editor: {}",
                if config.plugins.fs_editor {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            println!(
                "  terminal: {}",
                if config.plugins.terminal {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            println!(
                "  screenshot: {}",
                if config.plugins.screenshot {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            println!(
                "  git: {}",
                if config.plugins.git {
                    "enabled"
                } else {
                    "disabled"
                }
            );
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "plugins": [
                        {"name": "fs-editor", "enabled": config.plugins.fs_editor},
                        {"name": "terminal", "enabled": config.plugins.terminal},
                        {"name": "screenshot", "enabled": config.plugins.screenshot},
                        {"name": "git", "enabled": config.plugins.git},
                    ]
                }))?
            );
        }
    }

    Ok(())
}
