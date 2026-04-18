use std::path::PathBuf;
use std::{io, io::IsTerminal};

use anyhow::Result;

use crate::cli::commands::DaemonProfileArg;
use crate::cli::database_path::database_path;
use crate::config::{Config, DaemonProfile};
use crate::remote::RemoteManager;
use crate::storage::Database;
use crate::system::specs::SpecRepository;
use crate::system::{health, onboarding};

pub async fn handle_init(
    node_name: Option<String>,
    workspace: Option<PathBuf>,
    data_dir: Option<PathBuf>,
    profile: Option<DaemonProfileArg>,
    developer_mode: bool,
) -> Result<()> {
    let existed = Config::config_path()?.exists();
    let can_launch_wizard = should_launch_setup_wizard(
        existed,
        node_name.is_none(),
        workspace.is_none(),
        data_dir.is_none(),
        profile.is_none(),
        developer_mode,
        io::stdin().is_terminal(),
        io::stdout().is_terminal(),
    );

    if can_launch_wizard {
        println!("No existing Rove config found. Launching the first-run setup wizard.");
        return crate::cli::setup::handle_setup().await;
    }

    let mut config = Config::load_or_create()?;

    let requested_node_name = node_name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if let Some(workspace) = workspace {
        config.core.workspace = workspace;
    }
    if let Some(data_dir) = data_dir {
        config.core.data_dir = data_dir;
    }
    if let Some(profile) = profile {
        config.daemon.profile = match profile {
            DaemonProfileArg::Desktop => DaemonProfile::Desktop,
            DaemonProfileArg::Headless => DaemonProfile::Headless,
            DaemonProfileArg::Edge => DaemonProfile::Edge,
        };
        config.apply_profile_preset();
    }
    if developer_mode {
        config.daemon.developer_mode = true;
    }

    config.save()?;
    let config = Config::load_or_create()?;
    if let Some(node_name) = requested_node_name.as_deref() {
        RemoteManager::new(config.clone()).rename(node_name)?;
    }

    let database = Database::new(&database_path(&config)).await?;
    let repo = SpecRepository::new()?;
    let health = health::collect_snapshot(&config).await?;
    let onboarding = onboarding::collect(&config, &database, &health).await?;

    println!(
        "Initialized Rove {}",
        if existed { "layout" } else { "config" }
    );
    println!("Config: {}", Config::config_path()?.display());
    println!("Workspace: {}", config.core.workspace.display());
    println!("Data dir: {}", config.core.data_dir.display());
    println!("Agents dir: {}", repo.agents_dir().display());
    println!("Workflows dir: {}", repo.workflows_dir().display());
    println!(
        "Profile: {}{}",
        config.daemon.profile.as_str(),
        if config.daemon.developer_mode {
            " (developer mode)"
        } else {
            ""
        }
    );
    println!(
        "Health: {}",
        if health.healthy {
            "ready"
        } else {
            "needs attention"
        }
    );
    for issue in health.issues {
        println!("  - {}", issue);
    }
    println!();
    onboarding::print_text(&onboarding);

    Ok(())
}

fn should_launch_setup_wizard(
    config_exists: bool,
    no_node_name: bool,
    no_workspace: bool,
    no_data_dir: bool,
    no_profile: bool,
    developer_mode: bool,
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
) -> bool {
    !config_exists
        && no_node_name
        && no_workspace
        && no_data_dir
        && no_profile
        && !developer_mode
        && stdin_is_terminal
        && stdout_is_terminal
}

#[cfg(test)]
mod tests {
    use super::should_launch_setup_wizard;

    #[test]
    fn setup_wizard_launches_only_for_plain_first_run_in_terminal() {
        assert!(should_launch_setup_wizard(
            false, true, true, true, true, false, true, true
        ));
        assert!(!should_launch_setup_wizard(
            true, true, true, true, true, false, true, true
        ));
        assert!(!should_launch_setup_wizard(
            false, false, true, true, true, false, true, true
        ));
        assert!(!should_launch_setup_wizard(
            false, true, true, true, true, true, true, true
        ));
        assert!(!should_launch_setup_wizard(
            false, true, true, true, true, false, false, true
        ));
    }
}
