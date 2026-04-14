use std::path::PathBuf;

use anyhow::Result;

use crate::cli::commands::BackupAction;
use crate::config::Config;
use crate::system::backup::BackupManager;

pub fn handle_backup(action: BackupAction) -> Result<()> {
    let config = Config::load_or_create()?;
    let manager = BackupManager::new(config);
    match action {
        BackupAction::Export { path, force } => {
            let path = path.unwrap_or(manager.default_export_path()?);
            let manifest = manager.export(&path, force)?;
            println!("Exported backup to {}", path.display());
            for warning in manifest.warnings {
                println!("warning: {}", warning);
            }
            Ok(())
        }
        BackupAction::Restore { path, force } => restore_inner(path, force),
    }
}

pub fn handle_restore(path: PathBuf, force: bool) -> Result<()> {
    restore_inner(path, force)
}

fn restore_inner(path: PathBuf, force: bool) -> Result<()> {
    let config = Config::load_or_create()?;
    let manager = BackupManager::new(config);
    let manifest = manager.restore(&path, force)?;
    println!("Restored backup from {}", path.display());
    println!("Backup created at {}", manifest.created_at);
    for warning in manifest.warnings {
        println!("warning: {}", warning);
    }
    Ok(())
}
