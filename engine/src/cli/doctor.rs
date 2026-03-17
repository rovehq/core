use std::path::PathBuf;

use anyhow::Result;
use brain::reasoning::LocalBrain;
use serde_json::json;

use crate::cli::database_path::expand_data_dir;
use crate::config::Config;
use crate::platform::llama_search_paths;
use crate::security::crypto::CryptoModule;
use crate::steering::SteeringEngine;
use crate::storage::Database;
use crate::system::daemon::DaemonManager;

use super::output::OutputFormat;

pub async fn handle_doctor(config: &Config, format: OutputFormat) -> Result<()> {
    let mut issues = Vec::new();
    let mut checks = Vec::new();

    checks.push(("Configuration", "Valid".to_string()));
    checks.push(("Config path", Config::config_path()?.display().to_string()));
    checks.push((
        "Workspace path",
        config.core.workspace.display().to_string(),
    ));

    if config.core.workspace.exists() {
        checks.push(("Workspace directory", "Exists".to_string()));
    } else {
        checks.push(("Workspace directory", "Missing".to_string()));
        issues.push(format!(
            "Workspace directory does not exist: {:?}",
            config.core.workspace
        ));
    }

    let data_dir = expand_data_dir(&config.core.data_dir);
    checks.push(("Data directory path", data_dir.display().to_string()));
    if data_dir.exists() {
        checks.push(("Data directory", "Exists".to_string()));
    } else {
        checks.push(("Data directory", "Missing".to_string()));
        issues.push(format!("Data directory does not exist: {:?}", data_dir));
    }

    let db_path = data_dir.join("rove.db");
    checks.push(("Database path", db_path.display().to_string()));
    if db_path.exists() {
        checks.push(("Database", "Exists".to_string()));
        match Database::new(&db_path).await {
            Ok(_) => checks.push(("Database connection", "OK".to_string())),
            Err(error) => {
                checks.push(("Database connection", "Failed".to_string()));
                issues.push(format!("Cannot connect to database: {}", error));
            }
        }
    } else {
        checks.push(("Database", "Not initialized".to_string()));
        issues.push("Database not initialized. Run `rove setup` first.".to_string());
    }

    let llama_path = detect_llama_server_path();
    checks.push((
        "llama.cpp path",
        llama_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "Not found".to_string()),
    ));

    let local_brain = LocalBrain::new("http://localhost:8080", "qwen2.5-coder-0.5b");
    checks.push((
        "Brain status",
        if local_brain.check_available().await {
            format!("Running ({})", local_brain.model_name())
        } else if llama_path.is_some() {
            "Installed, server not running".to_string()
        } else {
            "Not installed".to_string()
        },
    ));

    checks.push(("Steering files", steering_summary(config).await));

    match DaemonManager::status(config) {
        Ok(status) => {
            checks.push((
                "Daemon",
                if status.is_running {
                    "Running".to_string()
                } else {
                    "Not running".to_string()
                },
            ));

            push_provider_check(
                &mut checks,
                "Ollama",
                status.providers.ollama,
                Some("Ollama is not running. Start Ollama to use local inference."),
                &mut issues,
            );
            push_provider_check(
                &mut checks,
                "OpenAI API key",
                status.providers.openai,
                None,
                &mut issues,
            );
            push_provider_check(
                &mut checks,
                "Anthropic API key",
                status.providers.anthropic,
                None,
                &mut issues,
            );
            push_provider_check(
                &mut checks,
                "Gemini API key",
                status.providers.gemini,
                None,
                &mut issues,
            );
            push_provider_check(
                &mut checks,
                "NVIDIA NIM API key",
                status.providers.nvidia_nim,
                None,
                &mut issues,
            );

            if !status.providers.ollama
                && !status.providers.openai
                && !status.providers.anthropic
                && !status.providers.gemini
                && !status.providers.nvidia_nim
            {
                issues.push(
                    "No LLM providers available. Configure at least one provider.".to_string(),
                );
            }
        }
        Err(error) => {
            checks.push(("Daemon status", "Error".to_string()));
            issues.push(format!("Cannot check daemon status: {}", error));
        }
    }

    checks.push(manifest_check(&mut issues));

    match format {
        OutputFormat::Text => print_text(&checks, &issues),
        OutputFormat::Json => print_json(&checks, &issues)?,
    }

    Ok(())
}

fn push_provider_check(
    checks: &mut Vec<(&'static str, String)>,
    name: &'static str,
    configured: bool,
    issue: Option<&str>,
    issues: &mut Vec<String>,
) {
    checks.push((
        name,
        if configured {
            "Configured".to_string()
        } else {
            "Not configured".to_string()
        },
    ));

    if !configured {
        if let Some(issue) = issue {
            issues.push(issue.to_string());
        }
    }
}

fn manifest_check(issues: &mut Vec<String>) -> (&'static str, String) {
    let manifest_paths = [
        PathBuf::from("manifest/manifest.json"),
        dirs::home_dir()
            .map(|home| home.join(".rove").join("manifest.json"))
            .unwrap_or_default(),
    ];

    let Some(manifest_path) = manifest_paths.iter().find(|path| path.exists()) else {
        return ("Manifest", "Not found".to_string());
    };

    let Ok(crypto) = CryptoModule::new() else {
        issues.push("Cannot initialize crypto module".to_string());
        return ("Manifest", "Crypto error".to_string());
    };

    let Ok(bytes) = std::fs::read(manifest_path) else {
        issues.push(format!("Cannot read manifest: {}", manifest_path.display()));
        return ("Manifest", "Unreadable".to_string());
    };

    match crypto.verify_manifest_file(&bytes) {
        Ok(()) => {
            let placeholder = serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|manifest| {
                    manifest
                        .get("signature")
                        .and_then(|value| value.as_str())
                        .map(str::to_string)
                })
                .map(|signature| {
                    signature.contains("PLACEHOLDER") || signature.contains("LOCAL_DEV")
                })
                .unwrap_or(false);

            if placeholder {
                ("Manifest signature", "Dev placeholder".to_string())
            } else {
                ("Manifest signature", "Valid".to_string())
            }
        }
        Err(_) => {
            issues.push("Manifest signature verification failed".to_string());
            ("Manifest signature", "Invalid".to_string())
        }
    }
}

fn print_text(checks: &[(&str, String)], issues: &[String]) {
    println!("Rove System Diagnostics");
    println!("=======================");
    println!();
    println!("System Checks:");
    for (check, status) in checks {
        println!("  {:<25} {}", format!("{}:", check), status);
    }
    println!();

    if issues.is_empty() {
        println!("All checks passed.");
        return;
    }

    println!("Issues found:");
    println!();
    for (index, issue) in issues.iter().enumerate() {
        println!("  {}. {}", index + 1, issue);
    }
}

fn print_json(checks: &[(&str, String)], issues: &[String]) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "checks": checks.iter().map(|(name, status)| {
                json!({
                    "name": name,
                    "status": status,
                })
            }).collect::<Vec<_>>(),
            "issues": issues,
            "healthy": issues.is_empty(),
        }))?
    );

    Ok(())
}

fn detect_llama_server_path() -> Option<PathBuf> {
    which::which("llama-server")
        .ok()
        .or_else(|| llama_search_paths().into_iter().find(|path| path.exists()))
}

async fn steering_summary(config: &Config) -> String {
    let mut parts = Vec::new();
    let global_dir = config.steering.skill_dir.clone();
    let workspace_dir = config.core.workspace.join(".rove").join("steering");

    match SteeringEngine::new(&global_dir).await {
        Ok(engine) => {
            let mut names: Vec<String> = engine
                .list_skills()
                .await
                .into_iter()
                .map(|skill| skill.name)
                .collect();
            names.sort();
            if names.is_empty() {
                parts.push(format!("global: none ({})", global_dir.display()));
            } else {
                parts.push(format!("global: {}", names.join(", ")));
            }
        }
        Err(error) => {
            parts.push(format!(
                "global: error loading {} ({})",
                global_dir.display(),
                error
            ));
        }
    }

    let workspace_files = list_steering_files(&workspace_dir);
    if workspace_files.is_empty() {
        parts.push(format!("workspace: none ({})", workspace_dir.display()));
    } else {
        parts.push(format!("workspace: {}", workspace_files.join(", ")));
    }

    parts.join(" | ")
}

fn list_steering_files(dir: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut files: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter_map(|path| {
            let ext = path.extension().and_then(|value| value.to_str())?;
            match ext {
                "toml" | "md" => path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .map(str::to_string),
                _ => None,
            }
        })
        .collect();
    files.sort();
    files
}
