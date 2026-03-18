use anyhow::{Context, Result};
use brain::reasoning::LocalBrain;

use super::server;

pub async fn run() -> Result<()> {
    let llama_path = which::which("llama-server").ok();
    let metadata = server::read_metadata()?;
    let port = metadata.as_ref().map(|meta| meta.port).unwrap_or(8080);
    let brain = LocalBrain::new(
        format!("http://localhost:{}", port),
        detect_model_name(&metadata),
    );
    let running = brain.check_available().await;
    let installed_models = installed_models()?;
    let adapter_path = LocalBrain::adapter_path().filter(|path| path.exists());

    println!("Local Brain Status");
    println!();
    println!(
        "llama.cpp: {}",
        llama_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "not found".to_string())
    );
    println!(
        "Server:    {}",
        if running { "running" } else { "not running" }
    );
    println!("Transport: HTTP http://localhost:{}", port);
    println!(
        "Model:     {}",
        metadata
            .as_ref()
            .map(|meta| meta.model_path.as_str())
            .or_else(|| installed_models.first().map(String::as_str))
            .unwrap_or("not installed")
    );
    println!(
        "LoRA:      {}",
        adapter_path
            .as_ref()
            .map(|path| format!("loaded ({})", path.display()))
            .unwrap_or_else(|| "not loaded".to_string())
    );
    println!(
        "Installed: {}",
        if installed_models.is_empty() {
            "none".to_string()
        } else {
            installed_models.join(", ")
        }
    );

    Ok(())
}

fn detect_model_name(metadata: &Option<server::ServerMetadata>) -> String {
    metadata
        .as_ref()
        .and_then(|meta| {
            std::path::Path::new(&meta.model_path)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn installed_models() -> Result<Vec<String>> {
    let brain_dir = LocalBrain::default_brain_dir().context("Failed to get brain directory")?;
    if !brain_dir.exists() {
        return Ok(Vec::new());
    }

    let entries = std::fs::read_dir(brain_dir).context("Failed to read brain directory")?;
    let mut models = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("gguf") {
            continue;
        }
        if path.file_name().and_then(|name| name.to_str()) == Some("adapter.gguf") {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
            models.push(name.to_string());
        }
    }

    models.sort();
    Ok(models)
}
