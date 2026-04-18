use anyhow::{anyhow, Result};

use crate::config::Config;
use crate::hooks::{HookManager, HookSummary};

use super::commands::HookAction;

pub async fn handle_hooks(action: HookAction, config: &Config) -> Result<()> {
    let manager = HookManager::discover(config);
    match action {
        HookAction::List => print_status(&manager.status().await),
        HookAction::Inspect { name } => {
            let hook = manager
                .inspect(&name)
                .await
                .ok_or_else(|| anyhow!("Hook '{}' not found", name))?;
            print_hook(&hook);
            Ok(())
        }
    }
}

fn print_status(status: &crate::hooks::HookStatus) -> Result<()> {
    if status.hooks.is_empty() {
        println!("No lifecycle hooks discovered.");
        return Ok(());
    }

    println!(
        "{:<24} {:<28} {:<9} {:<10} SOURCE",
        "NAME", "EVENTS", "DISABLED", "TIMEOUT"
    );
    for hook in &status.hooks {
        println!(
            "{:<24} {:<28} {:<9} {:<10} {}",
            hook.name,
            hook.events.join(","),
            if hook.disabled { "yes" } else { "no" },
            format!("{}s", hook.timeout_secs),
            hook.source_path
        );
    }
    Ok(())
}

fn print_hook(hook: &HookSummary) {
    println!("Name: {}", hook.name);
    if let Some(description) = &hook.description {
        println!("Description: {}", description);
    }
    println!("Events: {}", hook.events.join(", "));
    println!("Command: {}", hook.command);
    println!("Timeout: {}s", hook.timeout_secs);
    println!("Disabled: {}", if hook.disabled { "yes" } else { "no" });
    println!("Consecutive failures: {}", hook.consecutive_failures);
    println!("Source: {}", hook.source_path);
    if !hook.requires.os.is_empty() {
        println!("Requires OS: {}", hook.requires.os.join(", "));
    }
    if !hook.requires.bins.is_empty() {
        println!("Requires bins: {}", hook.requires.bins.join(", "));
    }
    if !hook.requires.env.is_empty() {
        println!("Requires env: {}", hook.requires.env.join(", "));
    }
}
