use std::path::PathBuf;

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
