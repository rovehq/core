use std::path::{Path, PathBuf};

use crate::config::Config;

pub fn database_path(config: &Config) -> PathBuf {
    expand_data_dir(&config.core.data_dir).join("rove.db")
}

pub fn expand_data_dir(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();

    if raw == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    }

    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }

    path.to_path_buf()
}
