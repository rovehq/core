use anyhow::Result;

use crate::cli::commands::{DaemonProfileArg, ServiceInstallModeArg, ServiceTarget};
use crate::config::Config;
use crate::config::DaemonProfile;
use crate::service_install::{ServiceInstallMode, ServiceInstaller};
use crate::services::{ManagedService, ServiceManager, ServiceStatus};

#[derive(Debug, Clone, Copy)]
pub enum ServiceAction {
    Enable,
    Disable,
    Show,
}

pub fn list(config: &Config) {
    println!("Services:");
    for status in ServiceManager::new(config.clone()).list() {
        println!(
            "- {} [{}]",
            status.name,
            if status.enabled {
                "enabled"
            } else {
                "disabled"
            }
        );
    }
}

pub fn handle(action: ServiceAction, target: ServiceTarget, config: &mut Config) -> Result<()> {
    let mut manager = ServiceManager::new(config.clone());
    let service = to_managed_service(target);

    match action {
        ServiceAction::Show => {
            let status = manager.describe(service);
            print_status(&status);
            Ok(())
        }
        ServiceAction::Enable => {
            let status = manager.set_enabled(service, true)?;
            *config = manager.into_config();
            println!("Enabled service '{}'.", status.name);
            if status.name == "connector-engine" && status.enabled {
                println!(
                    "Install or add connectors next with `rove connector add ...` or `rove connector install ...`."
                );
            }
            Ok(())
        }
        ServiceAction::Disable => {
            let status = manager.set_enabled(service, false)?;
            *config = manager.into_config();
            println!("Disabled service '{}'.", status.name);
            Ok(())
        }
    }
}

pub fn install_status(config: &Config) -> Result<()> {
    let status = ServiceInstaller::new(config.clone()).status()?;
    print_install_state("login", &status.login);
    print_install_state("boot", &status.boot);
    if let Some(binary) = status.current_binary {
        println!("binary: {}", binary);
    }
    println!("default_port: {}", status.default_port);
    Ok(())
}

pub fn install_service(
    mode: ServiceInstallModeArg,
    profile: Option<DaemonProfileArg>,
    port: u16,
    config: &Config,
) -> Result<()> {
    let installed = ServiceInstaller::new(config.clone()).install(
        to_service_install_mode(mode),
        profile.map(to_daemon_profile),
        port,
    )?;
    println!(
        "Installed {} service at {} with default profile {}.",
        installed.mode, installed.path, installed.default_profile
    );
    Ok(())
}

pub fn uninstall_service(mode: ServiceInstallModeArg, config: &Config) -> Result<()> {
    let mode_value = to_service_install_mode(mode);
    ServiceInstaller::new(config.clone()).uninstall(mode_value)?;
    println!("Removed {} service install.", mode_value.as_str());
    Ok(())
}

fn print_status(status: &ServiceStatus) {
    println!(
        "{}: {}",
        status.name,
        if status.enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    for (key, value) in &status.details {
        println!("{}: {}", key, value);
    }
}

fn print_install_state(label: &str, status: &crate::service_install::ServiceInstallState) {
    println!(
        "{}: {}{}",
        label,
        if status.installed {
            "installed"
        } else {
            "not installed"
        },
        if status.supported {
            ""
        } else {
            " (unsupported)"
        }
    );
    if status.supported {
        println!("  path: {}", status.path);
        println!("  default_profile: {}", status.default_profile);
        println!("  auto_restart: {}", status.auto_restart);
    }
}

fn to_managed_service(target: ServiceTarget) -> ManagedService {
    match target {
        ServiceTarget::Logging => ManagedService::Logging,
        ServiceTarget::Webui => ManagedService::WebUi,
        ServiceTarget::Remote => ManagedService::Remote,
        ServiceTarget::ConnectorEngine => ManagedService::ConnectorEngine,
    }
}

fn to_service_install_mode(mode: ServiceInstallModeArg) -> ServiceInstallMode {
    match mode {
        ServiceInstallModeArg::Login => ServiceInstallMode::Login,
        ServiceInstallModeArg::Boot => ServiceInstallMode::Boot,
    }
}

fn to_daemon_profile(profile: DaemonProfileArg) -> DaemonProfile {
    match profile {
        DaemonProfileArg::Desktop => DaemonProfile::Desktop,
        DaemonProfileArg::Headless => DaemonProfile::Headless,
    }
}
