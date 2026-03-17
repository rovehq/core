use anyhow::{Context, Result};
use brain::reasoning::LocalBrain;

pub fn run(model: &str) -> Result<()> {
    let brain_dir = LocalBrain::default_brain_dir().context("Failed to get brain directory")?;
    let model_path = brain_dir.join(format!("{}.gguf", model));

    if !model_path.exists() {
        println!("Model not found: {}", model);
        return Ok(());
    }

    std::fs::remove_file(&model_path).context("Failed to remove model file")?;
    println!("Removed model: {}", model);
    Ok(())
}
