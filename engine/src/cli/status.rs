use anyhow::Result;

use crate::config::metadata::{APP_DISPLAY_NAME, VERSION};
use crate::config::Config;
use crate::system::health;

pub async fn show() -> Result<()> {
    let config = Config::load_or_create()?;
    let snapshot = health::collect_snapshot(&config).await?;

    println!();
    println!("  {} v{}", APP_DISPLAY_NAME, VERSION);
    println!(
        "  Daemon: {}",
        if snapshot.daemon_running {
            snapshot
                .daemon_pid
                .map(|pid| format!("running (pid {pid})"))
                .unwrap_or_else(|| "running".to_string())
        } else {
            "stopped".to_string()
        }
    );
    println!(
        "  Auth:   {}",
        snapshot
            .auth
            .session_state
            .clone()
            .unwrap_or_else(|| snapshot.auth.password_state.clone())
    );
    println!(
        "  Control: {} · {}",
        snapshot.control_plane.control_url,
        if snapshot.control_plane.tls_enabled {
            "TLS"
        } else {
            "HTTP"
        }
    );
    println!(
        "  Bind:   configured {} · active {}",
        snapshot.control_plane.configured_bind_addr, snapshot.control_plane.listen_addr
    );
    println!(
        "  Runtime: {} profile · node {} · secrets {}",
        snapshot.profile, snapshot.node_name, snapshot.secret_backend
    );
    println!(
        "  Install: login {} · boot {}",
        install_label(&snapshot.service_install.login),
        install_label(&snapshot.service_install.boot)
    );

    if !snapshot.services.is_empty() {
        println!(
            "  Services: {}",
            status_list(snapshot.services.iter().map(|service| {
                format!(
                    "{}={}",
                    service.name,
                    if service.enabled { "on" } else { "off" }
                )
            }))
        );
    }

    if !snapshot.channels.is_empty() {
        println!(
            "  Channels: {}",
            status_list(snapshot.channels.iter().map(|channel| {
                format!(
                    "{}={}",
                    channel.name,
                    if channel.healthy {
                        "healthy"
                    } else if channel.enabled {
                        "needs-setup"
                    } else {
                        "off"
                    }
                )
            }))
        );
    }

    if !snapshot.transports.is_empty() {
        println!(
            "  Transports: {}",
            status_list(snapshot.transports.iter().map(|transport| {
                format!(
                    "{}={}",
                    transport.name,
                    if transport.healthy {
                        "healthy"
                    } else if transport.enabled {
                        "needs-attention"
                    } else {
                        "off"
                    }
                )
            }))
        );
    }

    println!(
        "  Paths:  data {} · db {} · log {}",
        path_label(snapshot.data_dir.exists, snapshot.data_dir.writable),
        path_label(snapshot.database.exists, snapshot.database.writable),
        path_label(snapshot.log_file.exists, snapshot.log_file.writable)
    );

    if !snapshot.issues.is_empty() {
        println!();
        println!("  Issues:");
        for issue in &snapshot.issues {
            println!("    - {}", issue);
        }
    }

    println!();
    Ok(())
}

fn install_label(state: &crate::system::service_install::ServiceInstallState) -> &'static str {
    if !state.supported {
        "unsupported"
    } else if state.installed {
        "installed"
    } else {
        "not-installed"
    }
}

fn path_label(exists: bool, writable: bool) -> &'static str {
    match (exists, writable) {
        (true, true) => "ready",
        (true, false) => "read-only",
        (false, true) => "creatable",
        (false, false) => "missing",
    }
}

fn status_list(items: impl Iterator<Item = String>) -> String {
    items.collect::<Vec<_>>().join(", ")
}
