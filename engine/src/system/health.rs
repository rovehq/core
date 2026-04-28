use std::fs;
use std::path::Path;

use anyhow::Result;
use sdk::AuthStatus;
use serde::Serialize;

use crate::channels::manager::ChannelManager;
use crate::cli::database_path::expand_data_dir;
use crate::config::Config;
use crate::remote::RemoteManager;
use crate::security::{password_protection_state, PasswordProtectionState};
use crate::services::{ServiceManager, ServiceStatus};
use crate::system::daemon::DaemonManager;
use crate::system::logs;
use crate::system::runtime_state;
use crate::system::service_install::{ServiceInstallStatus, ServiceInstaller};
use crate::zerotier::ZeroTierManager;

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
pub struct AuthHealthSummary {
    pub password_state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_expires_in_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absolute_expires_in_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ControlPlaneSummary {
    pub webui_enabled: bool,
    pub configured_bind_addr: String,
    pub listen_addr: String,
    pub port: u16,
    pub control_url: String,
    pub tls_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_binary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RemoteHealthSummary {
    pub enabled: bool,
    pub node_name: String,
    pub paired_nodes: usize,
    pub transport_count: usize,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct TransportHealthSummary {
    pub name: String,
    pub enabled: bool,
    pub configured: bool,
    pub healthy: bool,
    pub summary: String,
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
    pub auth: AuthHealthSummary,
    pub control_plane: ControlPlaneSummary,
    pub service_install: ServiceInstallStatus,
    pub services: Vec<ServiceStatus>,
    pub channels: Vec<crate::channels::manager::ChannelStatus>,
    pub transports: Vec<TransportHealthSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<RemoteHealthSummary>,
    pub checks: Vec<HealthCheckRecord>,
    pub issues: Vec<String>,
}

pub async fn collect_snapshot(config: &Config) -> Result<RuntimeHealthSnapshot> {
    collect_snapshot_with_auth(config, None).await
}

pub async fn collect_snapshot_with_auth(
    config: &Config,
    session_auth: Option<&AuthStatus>,
) -> Result<RuntimeHealthSnapshot> {
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

    let runtime_state = runtime_state::read(config)?
        .filter(|state| daemon_status.is_running && daemon_status.pid == state.pid);
    let tls_status = crate::api::server::tls::localhost_tls_status();

    let auth = auth_summary(config, session_auth);
    let control_plane = control_plane_summary(
        config,
        &service_install,
        daemon_status.is_running,
        runtime_state.as_ref(),
        tls_status.enabled,
    );

    let zerotier_status = ZeroTierManager::new(config.clone()).status().await.ok();
    let transports = vec![transport_summary(zerotier_status.as_ref())];

    let mut checks = Vec::new();
    let mut issues = Vec::new();

    push_path_check("Config file", &config_file, true, &mut checks, &mut issues);
    push_path_check("Workspace", &workspace, true, &mut checks, &mut issues);
    push_path_check(
        "Data directory",
        &data_dir_status,
        true,
        &mut checks,
        &mut issues,
    );
    push_path_check("Database", &database, false, &mut checks, &mut issues);
    push_path_check("Log file", &log_file, false, &mut checks, &mut issues);
    push_path_check(
        "Policy directory",
        &policy_dir_status,
        false,
        &mut checks,
        &mut issues,
    );

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

    checks.push(HealthCheckRecord {
        name: "Auth".to_string(),
        ok: auth.password_state != "uninitialized"
            && auth.password_state != "tampered"
            && auth.password_state != "unknown",
        detail: auth_detail(&auth),
    });
    if auth.password_state == "uninitialized" {
        issues.push(
            "Daemon password is not configured. Run `rove init` or finish first-run auth setup."
                .to_string(),
        );
    } else if auth.password_state == "tampered" {
        issues.push(
            "Daemon password integrity failed. Run `rove auth reset-password` on this machine."
                .to_string(),
        );
    } else if auth.password_state == "legacy_unsealed" {
        issues.push(
            "Daemon password is configured but not device-sealed yet. Run `rove auth reset-password` to harden it."
                .to_string(),
        );
    }

    checks.push(HealthCheckRecord {
        name: "Control plane".to_string(),
        ok: control_plane.webui_enabled,
        detail: format!(
            "{} · configured {}",
            control_plane.control_url, control_plane.configured_bind_addr
        ),
    });
    if !control_plane.webui_enabled {
        issues.push("Hosted WebUI control plane is disabled in config.".to_string());
    }

    for transport in &transports {
        checks.push(HealthCheckRecord {
            name: format!("Transport: {}", transport.name),
            ok: !transport.enabled || transport.healthy,
            detail: transport.summary.clone(),
        });
        if transport.enabled && !transport.healthy {
            issues.push(format!(
                "Transport '{}' needs attention: {}",
                transport.name, transport.summary
            ));
        }
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
        auth,
        control_plane,
        service_install,
        services,
        channels,
        transports,
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

fn auth_summary(config: &Config, session_auth: Option<&AuthStatus>) -> AuthHealthSummary {
    let password_state = match password_protection_state(config) {
        Ok(PasswordProtectionState::Uninitialized) => "uninitialized".to_string(),
        Ok(PasswordProtectionState::LegacyUnsealed) => "legacy_unsealed".to_string(),
        Ok(PasswordProtectionState::Sealed) => "device_sealed".to_string(),
        Ok(PasswordProtectionState::Tampered) => "tampered".to_string(),
        Err(_) => "unknown".to_string(),
    };

    AuthHealthSummary {
        password_state,
        session_state: session_auth.map(auth_state_label),
        idle_expires_in_secs: session_auth.and_then(|status| status.idle_expires_in_secs),
        absolute_expires_in_secs: session_auth.and_then(|status| status.absolute_expires_in_secs),
    }
}

fn auth_detail(auth: &AuthHealthSummary) -> String {
    match (auth.password_state.as_str(), auth.session_state.as_deref()) {
        ("uninitialized", _) => "password not configured".to_string(),
        ("tampered", _) => {
            "password integrity failed; reset is required on this device".to_string()
        }
        ("legacy_unsealed", Some(session_state)) => {
            format!("password configured without device seal, session {session_state}")
        }
        ("legacy_unsealed", None) => "password configured without device seal".to_string(),
        (state, Some(session_state)) => {
            format!(
                "password {}, session {session_state}",
                state.replace('_', " ")
            )
        }
        (state, None) => format!("password {}", state.replace('_', " ")),
    }
}

fn auth_state_label(status: &AuthStatus) -> String {
    match status.state {
        sdk::AuthState::Uninitialized => "uninitialized",
        sdk::AuthState::Locked => "locked",
        sdk::AuthState::Tampered => "tampered",
        sdk::AuthState::Unlocked => "unlocked",
        sdk::AuthState::ReauthRequired => "reauth_required",
    }
    .to_string()
}

fn control_plane_summary(
    config: &Config,
    service_install: &ServiceInstallStatus,
    daemon_running: bool,
    runtime_record: Option<&runtime_state::RuntimeStateRecord>,
    tls_enabled: bool,
) -> ControlPlaneSummary {
    let configured_port = runtime_state::configured_port(&config.webui.bind_addr);
    let port = runtime_record
        .map(|state| state.port)
        .unwrap_or(configured_port);
    let listen_addr = runtime_record
        .map(|state| state.listen_addr.clone())
        .unwrap_or_else(|| runtime_state::configured_listen_addr(&config.webui.bind_addr, port));
    let tls_enabled = runtime_record
        .map(|state| state.tls_enabled)
        .unwrap_or(tls_enabled);
    let control_url = runtime_record
        .map(|state| state.control_url.clone())
        .unwrap_or_else(|| runtime_state::control_url_for_text(&listen_addr, tls_enabled));

    ControlPlaneSummary {
        webui_enabled: config.webui.enabled,
        configured_bind_addr: config.webui.bind_addr.clone(),
        listen_addr: if daemon_running {
            listen_addr
        } else {
            runtime_state::configured_listen_addr(&config.webui.bind_addr, port)
        },
        port,
        control_url,
        tls_enabled,
        current_binary: service_install.current_binary.clone(),
    }
}

fn transport_summary(status: Option<&sdk::ZeroTierStatus>) -> TransportHealthSummary {
    let Some(status) = status else {
        return TransportHealthSummary {
            name: "zerotier".to_string(),
            enabled: false,
            configured: false,
            healthy: false,
            summary: "status unavailable".to_string(),
        };
    };

    let summary = if status.joined {
        let network = status
            .network_name
            .clone()
            .or_else(|| status.network_id.clone())
            .unwrap_or_else(|| "network".to_string());
        format!(
            "joined {network} · {} candidate(s) · sync {}",
            status.candidate_count, status.sync_state
        )
    } else if status.enabled {
        "enabled but not joined".to_string()
    } else {
        "disabled".to_string()
    };

    TransportHealthSummary {
        name: "zerotier".to_string(),
        enabled: status.enabled,
        configured: status.configured,
        healthy: status.service_online && (!status.enabled || status.joined),
        summary,
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
        return std::fs::OpenOptions::new().append(true).open(path).is_ok();
    }

    path.parent()
        .map(|parent| parent.exists() && directory_writable(parent))
        .unwrap_or(false)
}
