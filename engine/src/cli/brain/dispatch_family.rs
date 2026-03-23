use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Serialize;

const REQUIRED_FILES: &[&str] = &[
    "dispatch.onnx",
    "dispatch_labels.json",
    "dispatch_prototypes.json",
    "tokenizer.json",
];

#[derive(Debug, Clone, Serialize)]
pub struct DispatchBrainView {
    pub root: PathBuf,
    pub active: Option<String>,
    pub installed: Vec<String>,
    pub source: Option<String>,
}

pub async fn install(model: &str, source: Option<&Path>) -> Result<()> {
    let source = resolve_source(model, source)?;
    validate_artifacts_dir(&source)?;

    let root = dispatch_root()?;
    fs::create_dir_all(&root).with_context(|| format!("Failed to create '{}'", root.display()))?;
    let destination = root.join(model);
    if destination.exists() {
        fs::remove_dir_all(&destination).with_context(|| {
            format!(
                "Failed to replace existing dispatch model '{}'",
                destination.display()
            )
        })?;
    }
    copy_tree(&source, &destination)?;
    write_current_model(&root, model)?;

    println!("Installed dispatch brain '{}'.", model);
    println!("source: {}", source.display());
    println!("active: {}", model);
    Ok(())
}

pub fn list() -> Result<()> {
    let root = dispatch_root()?;
    let active = current_model_name(&root)?;
    let models = installed_models(&root)?;

    println!("Dispatch brain models");
    println!();
    if models.is_empty() {
        println!("No dispatch models installed.");
        if let Some(default_source) = default_source_dir() {
            println!();
            println!(
                "Install the current workspace artifacts with: rove brain install dispatch bert-tiny --source {}",
                default_source.display()
            );
        }
        return Ok(());
    }

    for model in models {
        let marker = if active.as_deref() == Some(model.as_str()) {
            "*"
        } else {
            " "
        };
        println!("{} {}", marker, model);
    }

    Ok(())
}

pub fn status() -> Result<()> {
    let view = status_view()?;

    println!("Dispatch Brain Status");
    println!();
    println!("root: {}", view.root.display());
    println!(
        "active: {}",
        view.active
            .clone()
            .unwrap_or_else(|| "not selected".to_string())
    );
    println!(
        "source: {}",
        view.source.unwrap_or_else(|| "not installed".to_string())
    );
    println!(
        "installed: {}",
        if view.installed.is_empty() {
            "none".to_string()
        } else {
            view.installed.join(", ")
        }
    );

    Ok(())
}

pub fn use_model(model: &str) -> Result<()> {
    let root = dispatch_root()?;
    let candidate = root.join(model);
    validate_artifacts_dir(&candidate).with_context(|| {
        format!(
            "Dispatch model '{}' is not installed under '{}'",
            model,
            root.display()
        )
    })?;
    write_current_model(&root, model)?;
    println!("Active dispatch brain set to '{}'.", model);
    Ok(())
}

pub fn remove(model: &str) -> Result<()> {
    let root = dispatch_root()?;
    let candidate = root.join(model);
    if !candidate.exists() {
        println!("Dispatch model not found: {}", model);
        return Ok(());
    }

    fs::remove_dir_all(&candidate)
        .with_context(|| format!("Failed to remove '{}'", candidate.display()))?;

    let active = current_model_name(&root)?;
    if active.as_deref() == Some(model) {
        let remaining = installed_models(&root)?;
        if let Some(next) = remaining.first() {
            write_current_model(&root, next)?;
        } else {
            clear_current_model(&root)?;
        }
    }

    println!("Removed dispatch brain '{}'.", model);
    Ok(())
}

pub fn status_view() -> Result<DispatchBrainView> {
    let root = dispatch_root()?;
    let active = current_model_name(&root)?;
    let installed = installed_models(&root)?;
    let active_path = active.as_ref().map(|model| root.join(model));
    let source = active_path
        .as_ref()
        .filter(|path| path.exists())
        .map(|path| path.display().to_string())
        .or_else(|| {
            std::env::var("ROVE_DISPATCH_ARTIFACTS")
                .ok()
                .filter(|path| !path.trim().is_empty())
        });

    Ok(DispatchBrainView {
        root,
        active,
        installed,
        source,
    })
}

fn dispatch_root() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not determine home directory")?;
    Ok(home.join(".rove").join("brains").join("dispatch"))
}

fn current_model_file(root: &Path) -> PathBuf {
    root.join("current-model")
}

fn current_model_name(root: &Path) -> Result<Option<String>> {
    let path = current_model_file(root);
    if !path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read '{}'", path.display()))?;
    let name = raw.trim();
    if name.is_empty() {
        Ok(None)
    } else {
        Ok(Some(name.to_string()))
    }
}

fn write_current_model(root: &Path, model: &str) -> Result<()> {
    fs::write(current_model_file(root), format!("{}\n", model))
        .with_context(|| format!("Failed to write '{}'", current_model_file(root).display()))
}

fn clear_current_model(root: &Path) -> Result<()> {
    let path = current_model_file(root);
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("Failed to remove '{}'", path.display()))?;
    }
    Ok(())
}

fn installed_models(root: &Path) -> Result<Vec<String>> {
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut models = Vec::new();
    for entry in
        fs::read_dir(root).with_context(|| format!("Failed to read '{}'", root.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if validate_artifacts_dir(&path).is_ok() {
            models.push(entry.file_name().to_string_lossy().to_string());
        }
    }
    models.sort();
    Ok(models)
}

fn resolve_source(model: &str, source: Option<&Path>) -> Result<PathBuf> {
    if let Some(source) = source {
        return Ok(source.to_path_buf());
    }

    let model_path = PathBuf::from(model);
    if model_path.exists() {
        return Ok(model_path);
    }

    if let Ok(env_path) = std::env::var("ROVE_DISPATCH_ARTIFACTS") {
        let candidate = PathBuf::from(env_path);
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    if let Some(default_source) = default_source_dir() {
        return Ok(default_source);
    }

    bail!(
        "Dispatch install needs an artifact directory. Use `rove brain install dispatch {} --source /path/to/artifacts`.",
        model
    )
}

fn default_source_dir() -> Option<PathBuf> {
    let candidate =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../brains/task-classifier/artifacts");
    candidate.exists().then_some(candidate)
}

fn validate_artifacts_dir(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!(
            "Dispatch artifact directory '{}' does not exist",
            path.display()
        );
    }
    if !path.is_dir() {
        bail!(
            "Dispatch artifact source '{}' is not a directory",
            path.display()
        );
    }
    for file in REQUIRED_FILES {
        let candidate = path.join(file);
        if !candidate.exists() {
            bail!("Missing dispatch artifact '{}'", candidate.display());
        }
    }
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)
        .with_context(|| format!("Failed to create '{}'", destination.display()))?;
    for entry in
        fs::read_dir(source).with_context(|| format!("Failed to read '{}'", source.display()))?
    {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path).with_context(|| {
                format!(
                    "Failed to copy '{}' to '{}'",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        }
    }
    Ok(())
}
