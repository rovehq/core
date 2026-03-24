use anyhow::Result;
use serde_json::json;

use crate::config::Config;
use crate::system::health::{self, RuntimeHealthSnapshot};

use super::output::OutputFormat;

pub async fn handle_doctor(config: &Config, format: OutputFormat) -> Result<()> {
    let snapshot = health::collect_snapshot(config).await?;

    match format {
        OutputFormat::Text => print_text(&snapshot),
        OutputFormat::Json => print_json(&snapshot)?,
    }

    Ok(())
}

fn print_text(snapshot: &RuntimeHealthSnapshot) {
    println!("Rove System Diagnostics");
    println!("=======================");
    println!();
    println!("System Checks:");
    for check in &snapshot.checks {
        println!(
            "  {:<25} {}",
            format!("{}:", check.name),
            if check.ok {
                check.detail.clone()
            } else {
                format!("{} [needs attention]", check.detail)
            }
        );
    }
    println!();

    if snapshot.issues.is_empty() {
        println!("All checks passed.");
        return;
    }

    println!("Issues found:");
    println!();
    for (index, issue) in snapshot.issues.iter().enumerate() {
        println!("  {}. {}", index + 1, issue);
    }
}

fn print_json(snapshot: &RuntimeHealthSnapshot) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "healthy": snapshot.healthy,
            "initialized": snapshot.initialized,
            "checks": snapshot.checks,
            "issues": snapshot.issues,
            "paths": {
                "config_file": snapshot.config_file,
                "workspace": snapshot.workspace,
                "data_dir": snapshot.data_dir,
                "database": snapshot.database,
                "log_file": snapshot.log_file,
                "policy_dir": snapshot.policy_dir,
            },
            "daemon_running": snapshot.daemon_running,
            "daemon_pid": snapshot.daemon_pid,
            "service_install": snapshot.service_install,
            "services": snapshot.services,
            "channels": snapshot.channels,
            "remote": snapshot.remote,
            "node_name": snapshot.node_name,
            "profile": snapshot.profile,
            "secret_backend": snapshot.secret_backend,
        }))?
    );

    Ok(())
}

