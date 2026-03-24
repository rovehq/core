use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::cli::database_path::expand_data_dir;
use crate::config::Config;
use crate::remote::RemoteManager;
use crate::system::daemon::DaemonManager;
use crate::system::logs;

const BACKUP_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub schema_version: u32,
    pub created_at: i64,
    pub rove_version: String,
    pub node_name: String,
    pub profile: String,
    pub secret_backend: String,
    pub config_path: String,
    pub data_dir: String,
    pub included_paths: Vec<String>,
    pub warnings: Vec<String>,
}

pub struct BackupManager {
    config: Config,
}

impl BackupManager {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub fn default_export_path(&self) -> Result<PathBuf> {
        let config_root = config_root()?;
        Ok(config_root
            .join("backups")
            .join(format!("backup-{}", Utc::now().format("%Y%m%d-%H%M%S"))))
    }

    pub fn export(&self, target: &Path, force: bool) -> Result<BackupManifest> {
        prepare_target_dir(target, force)?;

        let config_path = Config::config_path()?;
        let config_root = config_root()?;
        let data_dir = expand_data_dir(&self.config.core.data_dir);
        let mut included_paths = Vec::new();
        let mut warnings = Vec::new();

        if DaemonManager::status(&self.config)?.is_running {
            warnings.push(
                "Daemon was running during backup export; SQLite WAL sidecar files were included."
                    .to_string(),
            );
        }

        if self.config.secrets.backend.as_str() != "env" {
            warnings.push(
                "Secrets stored in keychain or vault backends are not exported by backup."
                    .to_string(),
            );
        }

        copy_file_if_exists(
            &config_path,
            &target.join("config").join("config.toml"),
            &mut included_paths,
        )?;
        copy_file_if_exists(
            &config_root.join("manifest.json"),
            &target.join("config").join("manifest.json"),
            &mut included_paths,
        )?;
        copy_dir_if_exists(
            &config_root.join("agents"),
            &target.join("specs").join("agents"),
            &mut included_paths,
        )?;
        copy_dir_if_exists(
            &config_root.join("workflows"),
            &target.join("specs").join("workflows"),
            &mut included_paths,
        )?;
        copy_dir_if_exists(
            self.config.policy.policy_dir(),
            &target.join("policy"),
            &mut included_paths,
        )?;

        for name in ["rove.db", "rove.db-wal", "rove.db-shm"] {
            copy_file_if_exists(&data_dir.join(name), &target.join("data").join(name), &mut included_paths)?;
        }
        copy_file_if_exists(
            &logs::log_file_path(),
            &target.join("logs").join("rove.log"),
            &mut included_paths,
        )?;

        let manifest = BackupManifest {
            schema_version: BACKUP_SCHEMA_VERSION,
            created_at: Utc::now().timestamp(),
            rove_version: crate::info::VERSION.to_string(),
            node_name: RemoteManager::new(self.config.clone())
                .status()
                .map(|status| status.node.node_name)
                .unwrap_or_else(|_| "local".to_string()),
            profile: self.config.daemon.profile.as_str().to_string(),
            secret_backend: self.config.secrets.backend.as_str().to_string(),
            config_path: config_path.display().to_string(),
            data_dir: data_dir.display().to_string(),
            included_paths,
            warnings,
        };

        let manifest_path = target.join("manifest.json");
        if let Some(parent) = manifest_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)
            .with_context(|| format!("Failed to write {}", manifest_path.display()))?;

        Ok(manifest)
    }

    pub fn restore(&self, source: &Path, force: bool) -> Result<BackupManifest> {
        if !source.exists() {
            bail!("Backup source does not exist: {}", source.display());
        }

        if DaemonManager::status(&self.config)?.is_running && !force {
            bail!(
                "Refusing to restore while the daemon is running. Stop it first or pass --force."
            );
        }

        let manifest = self.read_manifest(source)?;
        let config_path = Config::config_path()?;
        let config_root = config_root()?;
        let data_dir = expand_data_dir(&self.config.core.data_dir);

        copy_tree_into(
            &source.join("config"),
            &config_root,
            force,
        )?;
        copy_tree_into(
            &source.join("specs").join("agents"),
            &config_root.join("agents"),
            force,
        )?;
        copy_tree_into(
            &source.join("specs").join("workflows"),
            &config_root.join("workflows"),
            force,
        )?;
        copy_tree_into(&source.join("policy"), self.config.policy.policy_dir(), force)?;
        copy_tree_into(&source.join("data"), &data_dir, force)?;

        let config_from_backup = source.join("config").join("config.toml");
        if config_from_backup.exists() {
            if let Some(parent) = config_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create {}", parent.display()))?;
            }
            fs::copy(&config_from_backup, &config_path).with_context(|| {
                format!(
                    "Failed to restore config from {} to {}",
                    config_from_backup.display(),
                    config_path.display()
                )
            })?;
        }

        Ok(manifest)
    }

    pub fn read_manifest(&self, source: &Path) -> Result<BackupManifest> {
        let manifest_path = source.join("manifest.json");
        let raw = fs::read_to_string(&manifest_path)
            .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
        let manifest: BackupManifest = serde_json::from_str(&raw)
            .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;
        if manifest.schema_version > BACKUP_SCHEMA_VERSION {
            bail!(
                "Backup schema version {} is newer than this engine supports ({})",
                manifest.schema_version,
                BACKUP_SCHEMA_VERSION
            );
        }
        Ok(manifest)
    }
}

fn config_root() -> Result<PathBuf> {
    Config::config_path()?
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("Config path has no parent directory"))
}

fn prepare_target_dir(path: &Path, force: bool) -> Result<()> {
    if path.exists() {
        let is_empty = fs::read_dir(path)
            .with_context(|| format!("Failed to inspect {}", path.display()))?
            .next()
            .is_none();
        if !is_empty && !force {
            bail!(
                "Backup target already exists and is not empty: {}. Pass --force to reuse it.",
                path.display()
            );
        }
    }
    fs::create_dir_all(path).with_context(|| format!("Failed to create {}", path.display()))?;
    Ok(())
}

fn copy_file_if_exists(source: &Path, target: &Path, included_paths: &mut Vec<String>) -> Result<()> {
    if !source.exists() {
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    fs::copy(source, target).with_context(|| {
        format!("Failed to copy {} to {}", source.display(), target.display())
    })?;
    included_paths.push(target.display().to_string());
    Ok(())
}

fn copy_dir_if_exists(source: &Path, target: &Path, included_paths: &mut Vec<String>) -> Result<()> {
    if !source.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(source).with_context(|| format!("Failed to read {}", source.display()))? {
        let entry = entry?;
        let entry_path = entry.path();
        let target_path = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_if_exists(&entry_path, &target_path, included_paths)?;
        } else {
            copy_file_if_exists(&entry_path, &target_path, included_paths)?;
        }
    }

    Ok(())
}

fn copy_tree_into(source: &Path, target: &Path, force: bool) -> Result<()> {
    if !source.exists() {
        return Ok(());
    }
    fs::create_dir_all(target).with_context(|| format!("Failed to create {}", target.display()))?;

    for entry in fs::read_dir(source).with_context(|| format!("Failed to read {}", source.display()))? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree_into(&source_path, &target_path, force)?;
            continue;
        }

        if target_path.exists() && !force {
            bail!(
                "Restore target already exists: {}. Pass --force to overwrite backed-up files.",
                target_path.display()
            );
        }
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        fs::copy(&source_path, &target_path).with_context(|| {
            format!(
                "Failed to restore {} to {}",
                source_path.display(),
                target_path.display()
            )
        })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn export_and_restore_round_trip_selected_files() {
        let temp_dir = TempDir::new().unwrap();
        let config_root = temp_dir.path().join("config-root");
        let data_dir = temp_dir.path().join("data");
        std::env::set_var("ROVE_CONFIG_PATH", config_root.join("config.toml"));
        std::env::set_var("ROVE_DATA_DIR", &data_dir);

        let mut config = Config::load_or_create().unwrap();
        config.core.workspace = PathBuf::from(format!(
            "/tmp/rove-backup-test-workspace-{}",
            std::process::id()
        ));
        fs::create_dir_all(&config.core.workspace).unwrap();
        config.core.data_dir = data_dir.clone();
        config.policy.policy_dir = temp_dir.path().join("policy");
        fs::create_dir_all(&config.policy.policy_dir).unwrap();
        config.save().unwrap();

        let repo = crate::system::specs::SpecRepository::new().unwrap();
        repo.save_agent(&sdk::AgentSpec {
            id: "roundtrip".to_string(),
            name: "Roundtrip".to_string(),
            purpose: "Backup test".to_string(),
            instructions: "Verify backup restore.".to_string(),
            ..sdk::AgentSpec::default()
        })
        .unwrap();
        fs::create_dir_all(&data_dir).unwrap();
        fs::write(data_dir.join("rove.db"), "original-db").unwrap();

        let backup_dir = temp_dir.path().join("backup");
        let manager = BackupManager::new(Config::load_or_create().unwrap());
        manager.export(&backup_dir, false).unwrap();

        fs::write(repo.agents_dir().join("roundtrip.toml"), "corrupted").unwrap();
        fs::write(data_dir.join("rove.db"), "changed-db").unwrap();

        manager.restore(&backup_dir, true).unwrap();

        let restored_agent = fs::read_to_string(repo.agents_dir().join("roundtrip.toml")).unwrap();
        assert!(restored_agent.contains("Roundtrip"));
        assert_eq!(fs::read_to_string(data_dir.join("rove.db")).unwrap(), "original-db");

        std::env::remove_var("ROVE_CONFIG_PATH");
        std::env::remove_var("ROVE_DATA_DIR");
    }
}
