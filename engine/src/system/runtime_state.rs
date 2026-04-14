use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::cli::database_path::expand_data_dir;
use crate::config::{metadata::DEFAULT_PORT, Config};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStateRecord {
    pub pid: Option<u32>,
    pub bind_addr: String,
    pub listen_addr: String,
    pub port: u16,
    pub tls_enabled: bool,
    pub control_url: String,
    pub webui_enabled: bool,
    pub started_at: i64,
}

pub fn state_path(config: &Config) -> PathBuf {
    expand_data_dir(&config.core.data_dir).join("daemon-state.json")
}

pub fn read(config: &Config) -> Result<Option<RuntimeStateRecord>> {
    let path = state_path(config);
    if !path.exists() {
        return Ok(None);
    }

    let raw =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    let record = serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(Some(record))
}

pub fn write(config: &Config, record: &RuntimeStateRecord) -> Result<()> {
    let path = state_path(config);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let raw = serde_json::to_vec_pretty(record)?;
    fs::write(&path, raw).with_context(|| format!("Failed to write {}", path.display()))
}

pub fn clear(config: &Config) -> Result<()> {
    let path = state_path(config);
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("Failed to remove {}", path.display()))?;
    }
    Ok(())
}

pub fn build_record(
    bind_addr: &str,
    listen_addr: SocketAddr,
    port: u16,
    tls_enabled: bool,
    webui_enabled: bool,
) -> RuntimeStateRecord {
    RuntimeStateRecord {
        pid: Some(std::process::id()),
        bind_addr: bind_addr.to_string(),
        listen_addr: listen_addr.to_string(),
        port,
        tls_enabled,
        control_url: control_url_for_addr(&listen_addr, tls_enabled),
        webui_enabled,
        started_at: now_ts(),
    }
}

pub fn configured_port(bind_addr: &str) -> u16 {
    if let Ok(addr) = bind_addr.parse::<SocketAddr>() {
        return addr.port();
    }

    bind_addr
        .rsplit(':')
        .next()
        .and_then(|value| value.trim().parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT)
}

pub fn configured_listen_addr(bind_addr: &str, port: u16) -> String {
    daemon_socket_addr(bind_addr, port)
        .map(|addr| addr.to_string())
        .unwrap_or_else(|_| format!("127.0.0.1:{port}"))
}

pub fn control_url_for_text(listen_addr: &str, tls_enabled: bool) -> String {
    match listen_addr.parse::<SocketAddr>() {
        Ok(addr) => control_url_for_addr(&addr, tls_enabled),
        Err(_) => format!(
            "{}://localhost:{}",
            if tls_enabled { "https" } else { "http" },
            DEFAULT_PORT
        ),
    }
}

pub fn daemon_socket_addr(bind_addr: &str, port: u16) -> Result<SocketAddr> {
    if let Ok(mut addr) = bind_addr.parse::<SocketAddr>() {
        addr.set_port(port);
        return Ok(addr);
    }

    let host = bind_addr
        .split(':')
        .next()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("127.0.0.1");
    format!("{host}:{port}")
        .parse::<SocketAddr>()
        .with_context(|| format!("Invalid daemon bind address '{bind_addr}'"))
}

pub fn control_url_for_addr(addr: &SocketAddr, tls_enabled: bool) -> String {
    let scheme = if tls_enabled { "https" } else { "http" };
    let host = match addr.ip() {
        IpAddr::V4(ip) if ip.is_loopback() => "localhost".to_string(),
        IpAddr::V4(ip) if ip.is_unspecified() => Ipv4Addr::LOCALHOST.to_string(),
        IpAddr::V6(ip) if ip.is_unspecified() => Ipv4Addr::LOCALHOST.to_string(),
        IpAddr::V6(ip) if ip.is_loopback() => "localhost".to_string(),
        IpAddr::V6(ip) => format!("[{ip}]"),
        ip => ip.to_string(),
    };
    format!("{scheme}://{host}:{}", addr.port())
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
