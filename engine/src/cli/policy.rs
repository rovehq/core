use std::fs;
use std::path::PathBuf;

use anyhow::Result;

use crate::cli::commands::{PolicyAction, PolicyScopeArg};
use crate::config::Config;
use crate::policy::{explain_as_lines, PolicyManager};

pub async fn handle(action: PolicyAction, dir: Option<PathBuf>, config: &Config) -> Result<()> {
    let manager = PolicyManager::new(config.clone(), dir);

    match action {
        PolicyAction::List => list(&manager).await,
        PolicyAction::Status => status(&manager).await,
        PolicyAction::Show { name } => show(&manager, &name).await,
        PolicyAction::Enable { name } => {
            manager.enable(&name).await?;
            println!("Enabled policy '{}'.", name);
            Ok(())
        }
        PolicyAction::Disable { name } => {
            manager.disable(&name).await?;
            println!("Disabled policy '{}'.", name);
            Ok(())
        }
        PolicyAction::Default => {
            manager.bootstrap_defaults().await?;
            println!("Built-in policies are ready.");
            Ok(())
        }
        PolicyAction::Explain { task } | PolicyAction::Test { task } => {
            explain(&manager, &task.join(" ")).await
        }
        PolicyAction::Add { name, scope } => add(&manager, &name, scope).await,
        PolicyAction::Remove { name } => remove(&manager, &name).await,
    }
}

async fn list(manager: &PolicyManager) -> Result<()> {
    let all = manager.list().await?;
    if all.is_empty() {
        println!("No policies loaded.");
        return Ok(());
    }
    println!("Policies:");
    for policy in all {
        let active = if policy.active { "active" } else { "inactive" };
        println!(
            "- {} [{}] scope={} {}",
            policy.id,
            active,
            policy.scope,
            policy.path.display()
        );
    }
    Ok(())
}

async fn status(manager: &PolicyManager) -> Result<()> {
    let active = manager.active().await?;
    if active.is_empty() {
        println!("No active policies.");
        return Ok(());
    }
    println!("Active policies:");
    for policy in active {
        println!("- {}", policy);
    }
    Ok(())
}

async fn show(manager: &PolicyManager, name: &str) -> Result<()> {
    let policy = manager.get(name).await?;
    println!("id: {}", policy.id);
    println!("path: {}", policy.file_path.display());
    if let Some(config) = policy.config {
        println!("{}", toml::to_string_pretty(&config)?);
    } else {
        let content = fs::read_to_string(&policy.file_path)?;
        println!("{}", content);
    }
    Ok(())
}

async fn explain(manager: &PolicyManager, task: &str) -> Result<()> {
    let report = manager.explain(task).await?;
    for line in explain_as_lines(&report) {
        println!("{}", line);
    }
    Ok(())
}

async fn add(manager: &PolicyManager, name: &str, scope: PolicyScopeArg) -> Result<()> {
    let scope = match scope {
        PolicyScopeArg::User => sdk::PolicyScope::User,
        PolicyScopeArg::Workspace => sdk::PolicyScope::Workspace,
        PolicyScopeArg::Project => sdk::PolicyScope::Project,
    };
    let path = manager.add(name, scope).await?;
    println!("Created policy at {}", path.display());
    Ok(())
}

async fn remove(manager: &PolicyManager, name: &str) -> Result<()> {
    manager.remove(name).await?;
    println!("Removed policy '{}'.", name);
    Ok(())
}
