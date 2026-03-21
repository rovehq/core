use anyhow::Result;

use crate::cli::commands::ServiceTarget;
use crate::config::Config;
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
            if status.enabled { "enabled" } else { "disabled" }
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

fn print_status(status: &ServiceStatus) {
    println!(
        "{}: {}",
        status.name,
        if status.enabled { "enabled" } else { "disabled" }
    );
    for (key, value) in &status.details {
        println!("{}: {}", key, value);
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
