use std::path::Path;

use anyhow::{Context, Result};
use brain::reasoning::LocalBrain;

pub async fn run(model: &str) -> Result<()> {
    println!("Installing model: {}", model);
    println!();

    let brain_dir = LocalBrain::default_brain_dir().context("Failed to get brain directory")?;
    std::fs::create_dir_all(&brain_dir).context("Failed to create brain directory")?;

    println!("Brain directory: {}", brain_dir.display());

    let model_path = Path::new(model);
    if model_path.exists() && model_path.extension().and_then(|ext| ext.to_str()) == Some("gguf") {
        link_model(model_path, &brain_dir)?;
        println!();
        println!("Model installed successfully.");
        println!("Start llama-server with: rove brain start");
        return Ok(());
    }

    println!();
    println!("Download instructions:");
    println!();
    println!("Option 1: use an existing model");
    println!("  rove brain install /path/to/your/model.gguf");
    println!();
    println!("Option 2: download a GGUF from Hugging Face");
    println!("  wget https://huggingface.co/Qwen/Qwen2.5-Coder-0.5B-Instruct-GGUF/resolve/main/qwen2.5-coder-0.5b-instruct-q4_k_m.gguf");
    println!("  rove brain install qwen2.5-coder-0.5b-instruct-q4_k_m.gguf");
    println!();
    println!("Option 3: place the model manually under:");
    println!("  {}", brain_dir.display());

    Ok(())
}

fn link_model(model_path: &Path, brain_dir: &Path) -> Result<()> {
    let target = brain_dir.join(model_path.file_name().context("Invalid model filename")?);

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(model_path, &target).context("Failed to create symlink")?;
        println!("Linked model: {} -> {}", target.display(), model_path.display());
    }

    #[cfg(not(unix))]
    {
        std::fs::copy(model_path, &target).context("Failed to copy model")?;
        println!("Copied model: {} -> {}", model_path.display(), target.display());
    }

    Ok(())
}
