use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

pub fn log_file_path() -> PathBuf {
    if let Some(data_dir) = std::env::var_os("ROVE_DATA_DIR").filter(|value| !value.is_empty()) {
        let data_dir = PathBuf::from(data_dir);
        if let Some(parent) = data_dir.parent() {
            return parent.join("rove.log");
        }
        return data_dir.join("rove.log");
    }

    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".rove")
        .join("rove.log")
}

pub fn recent_lines(limit: usize) -> Result<Vec<String>> {
    let path = log_file_path();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let raw =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    let mut lines = raw.lines().map(ToOwned::to_owned).collect::<Vec<_>>();
    if lines.len() > limit {
        let drain = lines.len() - limit;
        lines.drain(0..drain);
    }
    Ok(lines)
}
