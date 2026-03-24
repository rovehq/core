use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::Serialize;

use crate::channels::manager::ChannelManager;
use crate::cli::database_path::expand_data_dir;
use crate::config::Config;
use crate::remote::RemoteManager;
use crate::services::{ServiceManager, ServiceStatus};
use crate::system::daemon::DaemonManager;
use crate::system::logs;
use crate::system::service_install::{ServiceInstallStatus, ServiceInstaller};

#[derive(Debug, Clone, Serialize, Default)]
pub struct PathStatus {
    pub path: String,
    pub exists: bool,
    pub writable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthCheckRecord {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RemoteHealthSummary {
    pub enabled: bool,
    pub node_name: String,
    pub paired_nodes: usize,
    pub transport_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeHealthSnapshot {
    pub healthy: bool,
    pub initialized: bool,
    pub config_file: PathStatus,
    pub workspace: PathStatus,
    pub data_dir: PathStatus,
    pub database: PathStatus,
    pub log_file: PathStatus,
    pub policy_dir: PathStatus,
    pub node_name: String,
    pub profile: String,
    pub secret_backend: String,
    pub daemon_running: bool,
    pub daemon_pid: Option<u32>,
    pub service_install: ServiceInstallStatus,
    pub services: Vec<ServiceStatus>,
    pub channels: Vec<crate::channels::manager::ChannelStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<RemoteHealthSummary>,
    pub checks: Vec<HealthCheckRecord>,
    pub issues: Vec<String>,
}

pub async fn collect_snapshot(config: &Config) -> Result<RuntimeHealthSnapshot> {
    let config_path = Config::config_path()?.to_path_buf();
    let data_dir = expand_data_dir(&config.core.data_dir);
    let db_path = data_dir.join("rove.db");
    let log_path = logs::log_file_path();
    let policy_dir = config.policy.policy_dir().clone();

    let config_file = file_status(&config_path);
    let workspace = directory_status(&config.core.workspace);
    let data_dir_status = directory_status(&data_dir);
    let database = file_status(&db_path);
    let log_file = file_status(&log_path);
    let policy_dir_status = directory_status(&policy_dir);

    let daemon_status = DaemonManager::status(config)?;
    let service_install = ServiceInstaller::new(config.clone()).status()?;
    let services = ServiceManager::new(config.clone()).list();
    let channels = ChannelManager::new(config.clone()).list().await?;
    let remote = RemoteManager::new(config.clone())
        .status()
        .ok()
        .map(|status| RemoteHealthSummary {
            enabled: config.ws_client.enabled || config.remote.transports.zerotier.enabled,
            node_name: status.node.node_name,
            paired_nodes: status.paired_nodes,
            transport_count: status.transports.len(),
        });

    let mut checks = Vec::new();
    let mut issues = Vec::new();

    push_path_check("Config file", &config_file, true, &mut checks, &mut issues);
    push_path_check("Workspace", &workspace, true, &mut checks, &mut issues);
    push_path_check("Data directory", &data_dir_status, true, &mut checks, &mut issues);
    push_path_check("Database", &database, false, &mut checks, &mut issues);
    push_path_check("Log file", &log_file, false, &mut checks, &mut issues);
    push_path_check("Policy directory", &policy_dir_status, false, &mut checks, &mut issues);

    checks.push(HealthCheckRecord {
        name: "Daemon".to_string(),
        ok: daemon_status.is_running,
        detail: if daemon_status.is_running {
            daemon_status
                .pid
                .map(|pid| format!("running (pid {pid})"))
                .unwrap_or_else(|| "running".to_string())
        } else {
            "not running".to_string()
        },
    });
    if !daemon_status.is_running {
        issues.push("Daemon is not running. Start it with `rove start`.".to_string());
    }

    for (name, configured) in [
        ("Ollama", daemon_status.providers.ollama),
        ("OpenAI", daemon_status.providers.openai),
        ("Anthropic", daemon_status.providers.anthropic),
        ("Gemini", daemon_status.providers.gemini),
        ("NVIDIA NIM", daemon_status.providers.nvidia_nim),
    ] {
        checks.push(HealthCheckRecord {
            name: format!("Provider: {name}"),
            ok: configured,
            detail: if configured {
                "configured".to_string()
            } else {
                "not configured".to_string()
            },
        });
    }

    if !daemon_status.providers.ollama
        && !daemon_status.providers.openai
        && !daemon_status.providers.anthropic
        && !daemon_status.providers.gemini
        && !daemon_status.providers.nvidia_nim
    {
        issues.push("No LLM providers are configured.".to_string());
    }

    let initialized = config_file.exists && data_dir_status.exists && database.exists;

    Ok(RuntimeHealthSnapshot {
        healthy: issues.is_empty(),
        initialized,
        config_file,
        workspace,
        data_dir: data_dir_status,
        database,
        log_file,
        policy_dir: policy_dir_status,
        node_name: remote
            .as_ref()
            .map(|item| item.node_name.clone())
            .unwrap_or_else(|| "local".to_string()),
        profile: config.daemon.profile.as_str().to_string(),
        secret_backend: config.secrets.backend.as_str().to_string(),
        daemon_running: daemon_status.is_running,
        daemon_pid: daemon_status.pid,
        service_install,
        services,
        channels,
        remote,
        checks,
        issues,
    })
}

fn push_path_check(
    name: &str,
    status: &PathStatus,
    expect_exists: bool,
    checks: &mut Vec<HealthCheckRecord>,
    issues: &mut Vec<String>,
) {
    let ok = status.writable && (!expect_exists || status.exists);
    let detail = format!(
        "{}{}",
        if status.exists { "exists" } else { "missing" },
        if status.writable {
            ", writable"
        } else {
            ", not writable"
        }
    );
    checks.push(HealthCheckRecord {
        name: name.to_string(),
        ok,
        detail,
    });
    if expect_exists && !status.exists {
        issues.push(format!("{name} is missing: {}", status.path));
    }
    if !status.writable {
        issues.push(format!("{name} is not writable: {}", status.path));
    }
}

fn directory_status(path: &Path) -> PathStatus {
    PathStatus {
        path: path.display().to_string(),
        exists: path.exists(),
        writable: directory_writable(path),
    }
}

fn file_status(path: &Path) -> PathStatus {
    PathStatus {
        path: path.display().to_string(),
        exists: path.exists(),
        writable: file_writable(path),
    }
}

fn directory_writable(path: &Path) -> bool {
    if path.exists() {
        if !path.is_dir() {
            return false;
        }
        let probe = path.join(format!(".rove-write-check-{}", std::process::id()));
        return fs::write(&probe, b"probe")
            .and_then(|_| fs::remove_file(&probe))
            .is_ok();
    }

    path.parent()
        .map(|parent| parent.exists() && directory_writable(parent))
        .unwrap_or(false)
}

fn file_writable(path: &Path) -> bool {
    if path.exists() {
        return std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .is_ok();
    }

    path.parent()
        .map(|parent| parent.exists() && directory_writable(parent))
        .unwrap_or(false)
}
