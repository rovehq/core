use anyhow::{Context, Result};
use brain::reasoning::LocalBrain;

pub fn run() -> Result<()> {
    println!("Installed Models");
    println!();

    let Some(brain_dir) = LocalBrain::default_brain_dir() else {
        println!("Could not determine brain directory");
        return Ok(());
    };

    if !brain_dir.exists() {
        println!("No models installed yet.");
        println!();
        println!("Install a model with: rove brain install qwen2.5-coder-0.5b");
        return Ok(());
    }

    let entries = std::fs::read_dir(&brain_dir).context("Failed to read brain directory")?;
    let mut found_model = false;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("gguf") {
            continue;
        }

        let name = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("unknown");
        if name == "adapter" {
            continue;
        }

        println!("  - {}", name);
        found_model = true;
    }

    if !found_model {
        println!("No models found.");
    }

    if let Some(adapter_path) = LocalBrain::adapter_path() {
        if adapter_path.exists() {
            println!();
            println!("LoRA adapter: {}", adapter_path.display());
        }
    }

    Ok(())
}
