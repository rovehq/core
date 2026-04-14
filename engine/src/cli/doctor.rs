use anyhow::Result;
use serde_json::json;

use crate::cli::database_path::database_path;
use crate::config::Config;
use crate::storage::Database;
use crate::system::{
    health::{self, RuntimeHealthSnapshot},
    onboarding::{self, OnboardingChecklist},
};

use super::output::OutputFormat;

pub async fn handle_doctor(config: &Config, format: OutputFormat) -> Result<()> {
    let snapshot = health::collect_snapshot(config).await?;
    let database = Database::new(&database_path(config)).await?;
    let onboarding = onboarding::collect(config, &database, &snapshot).await?;

    match format {
        OutputFormat::Text => print_text_with_security(&snapshot, &onboarding, config),
        OutputFormat::Json => print_json_with_security(&snapshot, &onboarding, config)?,
    }

    Ok(())
}

fn print_text(snapshot: &RuntimeHealthSnapshot, onboarding: &OnboardingChecklist) {
    println!("Rove System Diagnostics");
    println!("=======================");
    println!();
    println!(
        "Runtime truth: {} · {} · auth {}",
        snapshot.control_plane.control_url,
        snapshot.profile,
        snapshot
            .auth
            .session_state
            .as_deref()
            .unwrap_or(snapshot.auth.password_state.as_str())
    );
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
    } else {
        println!("Issues found:");
        println!();
        for (index, issue) in snapshot.issues.iter().enumerate() {
            println!("  {}. {}", index + 1, issue);
        }
    }

    println!();
    onboarding::print_text(onboarding);
}

fn print_text_with_security(
    snapshot: &RuntimeHealthSnapshot,
    onboarding: &OnboardingChecklist,
    config: &Config,
) {
    print_text(snapshot, onboarding);

    // Security posture summary
    println!();
    println!("Security Posture:");
    println!("  Approval mode:    {}", config.approvals.mode.as_str());
    println!("  Max risk tier:    {}", config.security.max_risk_tier);
    println!(
        "  Tier 1 confirm:   {}",
        if config.security.confirm_tier1 {
            "yes"
        } else {
            "no"
        }
    );
    println!(
        "  Tier 2 explicit:  {}",
        if config.security.require_explicit_tier2 {
            "yes"
        } else {
            "no"
        }
    );
    println!("  Secret backend:   {}", config.secrets.backend.as_str());
}

fn print_json_with_security(
    snapshot: &RuntimeHealthSnapshot,
    onboarding: &OnboardingChecklist,
    config: &Config,
) -> Result<()> {
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
            "auth": snapshot.auth,
            "control_plane": snapshot.control_plane,
            "service_install": snapshot.service_install,
            "services": snapshot.services,
            "channels": snapshot.channels,
            "transports": snapshot.transports,
            "remote": snapshot.remote,
            "node_name": snapshot.node_name,
            "profile": snapshot.profile,
            "secret_backend": snapshot.secret_backend,
            "security": {
                "approval_mode": config.approvals.mode.as_str(),
                "max_risk_tier": config.security.max_risk_tier,
                "confirm_tier1": config.security.confirm_tier1,
                "tier1_delay": config.security.confirm_tier1_delay,
                "require_explicit_tier2": config.security.require_explicit_tier2,
            },
            "onboarding": onboarding,
        }))?
    );

    Ok(())
}
