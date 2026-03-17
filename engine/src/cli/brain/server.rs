use anyhow::{Context, Result};
use brain::reasoning::LocalBrain;
use std::process::Command;

pub fn start(model: Option<&str>, port: u16) -> Result<()> {
    println!("Starting llama-server...");
    println!();

    if which::which("llama-server").is_err() {
        println!("llama-server not found in PATH");
        println!();
        println!("Run `rove brain setup` for installation instructions.");
        return Ok(());
    }

    let brain_dir = LocalBrain::default_brain_dir().context("Failed to get brain directory")?;
    let model_name = resolve_model_name(&brain_dir, model)?;
    let model_path = brain_dir.join(format!("{}.gguf", model_name));

    if !model_path.exists() {
        println!("Model not found: {}", model_name);
        return Ok(());
    }

    let mut command = Command::new("llama-server");
    command.arg("--model").arg(&model_path);
    command.arg("--port").arg(port.to_string());

    if let Some(adapter_path) = LocalBrain::adapter_path() {
        if adapter_path.exists() {
            println!("LoRA: {}", adapter_path.display());
            command.arg("--lora").arg(&adapter_path);
        }
    }

    println!("Model: {}", model_path.display());
    println!("Port:  {}", port);
    println!();
    println!("Press Ctrl+C to stop.");
    println!();

    let status = command.status().context("Failed to start llama-server")?;
    if !status.success() {
        println!("llama-server exited with an error");
    }

    Ok(())
}

pub fn stop() -> Result<()> {
    println!("Stopping llama-server...");
    println!();

    #[cfg(unix)]
    {
        let output = Command::new("pkill")
            .arg("llama-server")
            .output()
            .context("Failed to run pkill")?;

        if output.status.success() {
            println!("llama-server stopped");
        } else {
            println!("No running llama-server found");
        }
    }

    #[cfg(not(unix))]
    {
        println!("Stop is not implemented on this platform.");
        println!("Stop llama-server manually.");
    }

    Ok(())
}

fn resolve_model_name(brain_dir: &std::path::Path, model: Option<&str>) -> Result<String> {
    if let Some(model) = model {
        return Ok(model.to_string());
    }

    let entries = std::fs::read_dir(brain_dir).context("Failed to read brain directory")?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("gguf") {
            continue;
        }

        let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if name != "adapter" {
            return Ok(name.to_string());
        }
    }

    anyhow::bail!("No models found. Install a model with `rove brain install <model>`.")
}
