use anyhow::Result;
use sdk::{BrowserApprovalControls, BrowserProfileInput, BrowserProfileMode, BrowserRuntimeStatus};

use crate::cli::database_path::database_path;
use crate::config::Config;
use crate::runtime::RuntimeManager;
use crate::storage::Database;
use crate::system::browser::BrowserManager;

use super::commands::{
    BrowserAction, BrowserControlsAction, BrowserProfileAction, BrowserProfileModeArg,
};

pub async fn handle_browser(action: BrowserAction) -> Result<()> {
    let config = Config::load_or_create()?;
    let manager = BrowserManager::new(config);
    let runtime = current_runtime_status()
        .await
        .unwrap_or_else(|error| BrowserRuntimeStatus {
            warnings: vec![format!(
                "Failed to inspect current runtime browser backend: {}",
                error
            )],
            ..BrowserRuntimeStatus::default()
        });

    match action {
        BrowserAction::Status => print_status(manager.status_with_runtime(runtime)),
        BrowserAction::Enable => print_status(manager.set_enabled(true)?),
        BrowserAction::Disable => print_status(manager.set_enabled(false)?),
        BrowserAction::Controls { action } => match action {
            BrowserControlsAction::Show => print_status(manager.status_with_runtime(runtime)),
            BrowserControlsAction::Set {
                require_managed_launch_approval,
                require_existing_session_approval,
                require_remote_cdp_approval,
            } => {
                let current = manager.status();
                let controls = BrowserApprovalControls {
                    require_approval_for_managed_launch: require_managed_launch_approval
                        .unwrap_or(current.controls.require_approval_for_managed_launch),
                    require_approval_for_existing_session_attach: require_existing_session_approval
                        .unwrap_or(
                            current
                                .controls
                                .require_approval_for_existing_session_attach,
                        ),
                    require_approval_for_remote_cdp: require_remote_cdp_approval
                        .unwrap_or(current.controls.require_approval_for_remote_cdp),
                };
                print_status(manager.set_controls(controls)?);
            }
        },
        BrowserAction::Profile { action } => match action {
            BrowserProfileAction::List => print_status(manager.status_with_runtime(runtime)),
            BrowserProfileAction::Add {
                id,
                name,
                backend,
                mode,
                browser,
                user_data_dir,
                startup_url,
                cdp_url,
                notes,
                default,
                disabled,
            } => {
                let profile = BrowserProfileInput {
                    id,
                    name,
                    enabled: !disabled,
                    backend,
                    mode: parse_mode(mode),
                    browser,
                    user_data_dir,
                    startup_url,
                    cdp_url,
                    notes,
                };
                print_status(manager.upsert_profile(profile, default)?);
            }
            BrowserProfileAction::Remove { id } => print_status(manager.remove_profile(&id)?),
            BrowserProfileAction::Default { id } => print_status(manager.set_default_profile(&id)?),
        },
    }

    Ok(())
}

async fn current_runtime_status() -> Result<BrowserRuntimeStatus> {
    let config = Config::load_or_create()?;
    let database = Database::new(&database_path(&config)).await?;
    let runtime = RuntimeManager::build(&database, &config).await?;
    Ok(runtime.registry.browser_runtime_status().await)
}

fn parse_mode(mode: BrowserProfileModeArg) -> BrowserProfileMode {
    match mode {
        BrowserProfileModeArg::ManagedLocal => BrowserProfileMode::ManagedLocal,
        BrowserProfileModeArg::AttachExisting => BrowserProfileMode::AttachExisting,
        BrowserProfileModeArg::RemoteCdp => BrowserProfileMode::RemoteCdp,
    }
}

fn print_status(status: sdk::BrowserSurfaceStatus) {
    println!(
        "Browser surface: {}",
        if status.enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "Default profile: {}",
        status
            .default_profile_id
            .as_deref()
            .unwrap_or("not selected")
    );
    println!(
        "Approvals: managed_launch={} attach_existing={} remote_cdp={}",
        status.controls.require_approval_for_managed_launch,
        status.controls.require_approval_for_existing_session_attach,
        status.controls.require_approval_for_remote_cdp
    );
    println!(
        "Runtime: {}{}{}",
        if status.runtime.registered {
            status.runtime.backend_name.as_deref().unwrap_or("loaded")
        } else {
            "not loaded"
        },
        status
            .runtime
            .source
            .as_deref()
            .map(|source| format!(" [{}]", source))
            .unwrap_or_default(),
        if status.runtime.connected {
            " connected"
        } else if status.runtime.registered {
            " idle"
        } else {
            ""
        }
    );

    if !status.warnings.is_empty() {
        println!("Warnings:");
        for warning in &status.warnings {
            println!("- {}", warning);
        }
    }
    for warning in &status.runtime.warnings {
        println!("- {}", warning);
    }

    if status.profiles.is_empty() {
        println!("Profiles: none configured");
        return;
    }

    println!("Profiles:");
    for profile in status.profiles {
        println!(
            "- {} ({}) [{}] {}{}",
            profile.name,
            profile.id,
            profile.mode.as_str(),
            profile.readiness.as_str(),
            if profile.is_default { " default" } else { "" }
        );
        println!(
            "  enabled={} approval_required={}",
            profile.enabled, profile.approval_required
        );
        if let Some(backend) = &profile.backend {
            println!("  backend={}", backend);
        }
        if let Some(browser) = &profile.browser {
            println!("  browser={}", browser);
        }
        if let Some(user_data_dir) = &profile.user_data_dir {
            println!("  user_data_dir={}", user_data_dir);
        }
        if let Some(startup_url) = &profile.startup_url {
            println!("  startup_url={}", startup_url);
        }
        if let Some(cdp_url) = &profile.cdp_url {
            println!("  cdp_url={}", cdp_url);
        }
        if let Some(notes) = &profile.notes {
            println!("  notes={}", notes);
        }
        for warning in &profile.warnings {
            println!("  warning: {}", warning);
        }
    }
}
