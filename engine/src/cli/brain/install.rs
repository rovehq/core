use std::path::{Path, PathBuf};
use std::process::Command;

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

    if let Some(download) = download_spec(model) {
        let target = brain_dir.join(download.install_name);
        if target.exists() {
            println!("Model already installed: {}", target.display());
            return Ok(());
        }

        println!("Downloading from {}", download.url);
        download_with_curl(&download.url, &target)?;
        println!("Saved model to {}", target.display());
        println!("Start llama-server with: rove brain start");
        return Ok(());
    }

    println!();
    println!("Download instructions:");
    println!();
    println!("Option 1: use an existing model");
    println!("  rove brain install /path/to/your/model.gguf");
    println!();
    println!("Option 2: install the supported alias");
    println!("  rove brain install qwen2.5-coder-0.5b");
    println!();
    println!("Option 3: pass a direct model URL");
    println!("  rove brain install https://.../model.gguf");
    println!();
    println!("Option 4: place the model manually under:");
    println!("  {}", brain_dir.display());

    Ok(())
}

fn link_model(model_path: &Path, brain_dir: &Path) -> Result<()> {
    let target = brain_dir.join(model_path.file_name().context("Invalid model filename")?);
    if target.exists() {
        std::fs::remove_file(&target).ok();
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(model_path, &target).context("Failed to create symlink")?;
        println!(
            "Linked model: {} -> {}",
            target.display(),
            model_path.display()
        );
    }

    #[cfg(not(unix))]
    {
        std::fs::copy(model_path, &target).context("Failed to copy model")?;
        println!(
            "Copied model: {} -> {}",
            model_path.display(),
            target.display()
        );
    }

    Ok(())
}

struct DownloadSpec {
    url: String,
    install_name: String,
}

fn download_spec(model: &str) -> Option<DownloadSpec> {
    match model {
        "qwen2.5-coder-0.5b" | "qwen2.5-coder-0.5b.gguf" => Some(DownloadSpec {
            url: "https://huggingface.co/Qwen/Qwen2.5-Coder-0.5B-Instruct-GGUF/resolve/main/qwen2.5-coder-0.5b-instruct-q4_k_m.gguf".to_string(),
            install_name: "qwen2.5-coder-0.5b.gguf".to_string(),
        }),
        direct if direct.starts_with("https://") || direct.starts_with("http://") => {
            let file_name = direct.rsplit('/').next()?;
            if !file_name.ends_with(".gguf") {
                return None;
            }
            Some(DownloadSpec {
                url: direct.to_string(),
                install_name: file_name.to_string(),
            })
        }
        _ => None,
    }
}

fn download_with_curl(url: &str, target: &PathBuf) -> Result<()> {
    let status = Command::new("curl")
        .args(["-L", "--fail", "--progress-bar", "-o"])
        .arg(target)
        .arg(url)
        .status()
        .context("Failed to launch curl")?;

    if !status.success() {
        anyhow::bail!("curl exited with status {}", status);
    }

    Ok(())
}
