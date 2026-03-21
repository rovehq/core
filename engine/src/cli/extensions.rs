use anyhow::{bail, Result};

use crate::cli::commands::{ExtensionAction, PluginScaffoldType};
use crate::cli::output::OutputFormat;
use crate::config::Config;
use crate::runtime::PluginType;

pub enum ExtensionSurface {
    Skill,
    System,
    Channel,
}

impl ExtensionSurface {
    fn noun(&self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::System => "system",
            Self::Channel => "channel",
        }
    }

    fn scaffold_type(&self) -> PluginScaffoldType {
        match self {
            Self::Skill => PluginScaffoldType::Skill,
            Self::System => PluginScaffoldType::System,
            Self::Channel => PluginScaffoldType::Channel,
        }
    }

    fn expected_type(&self) -> PluginType {
        match self {
            Self::Skill => PluginType::Skill,
            Self::System => PluginType::Workspace,
            Self::Channel => PluginType::Channel,
        }
    }
}

pub async fn handle(
    config: &Config,
    surface: ExtensionSurface,
    action: ExtensionAction,
) -> Result<()> {
    if matches!(surface, ExtensionSurface::System) && matches!(action, ExtensionAction::List) {
        print_builtin_systems(config);
    }

    if matches!(surface, ExtensionSurface::System)
        && !matches!(action, ExtensionAction::List)
    {
        if let Some(result) = handle_builtin_system_action(config, &action)? {
            return Ok(result);
        }
    }

    match action {
        ExtensionAction::New { name } => {
            crate::cli::plugins::handle_new(&name, surface.scaffold_type()).await
        }
        ExtensionAction::Test {
            source,
            tool,
            input,
            files,
            args,
            no_build,
        } => {
            crate::cli::plugins::handle_test(
                source.as_deref(),
                tool.as_deref(),
                input.as_deref(),
                &files,
                &args,
                no_build,
            )
            .await
        }
        ExtensionAction::Pack {
            source,
            out,
            no_build,
        } => crate::cli::plugins::handle_pack(source.as_deref(), out.as_deref(), no_build).await,
        ExtensionAction::Publish {
            source,
            registry_dir,
            no_build,
        } => crate::cli::plugins::handle_publish(source.as_deref(), &registry_dir, no_build).await,
        ExtensionAction::Install {
            source,
            registry,
            version,
        } => {
            let expected_type = surface.expected_type();
            let installed = crate::cli::plugins::install_checked(
                config,
                &source,
                registry.as_deref(),
                version.as_deref(),
                Some(expected_type),
            )
            .await?;
            println!(
                "Installed {} '{}' [{}] version={}",
                surface.noun(),
                installed.name,
                installed.id,
                installed.version
            );
            Ok(())
        }
        ExtensionAction::Upgrade {
            source,
            registry,
            version,
        } => {
            let expected_type = surface.expected_type();
            let installed = crate::cli::plugins::upgrade_checked(
                config,
                &source,
                registry.as_deref(),
                version.as_deref(),
                Some(expected_type),
            )
            .await?;
            println!(
                "Upgraded {} '{}' [{}] to version {}",
                surface.noun(),
                installed.name,
                installed.id,
                installed.version
            );
            Ok(())
        }
        ExtensionAction::List => {
            crate::cli::plugins::handle_list_filtered(config, OutputFormat::Text, surface.noun())
                .await
        }
        ExtensionAction::Inspect { name } => {
            crate::cli::plugins::handle_inspect_filtered(config, &name, surface.noun()).await
        }
        ExtensionAction::Enable { name } => {
            crate::cli::plugins::handle_set_enabled_filtered(config, &name, true, surface.noun())
                .await
        }
        ExtensionAction::Disable { name } => {
            crate::cli::plugins::handle_set_enabled_filtered(config, &name, false, surface.noun())
                .await
        }
        ExtensionAction::Remove { name } => {
            crate::cli::plugins::handle_remove_filtered(config, &name, surface.noun()).await
        }
    }
}

fn handle_builtin_system_action(
    config: &Config,
    action: &ExtensionAction,
) -> Result<Option<()>> {
    match action {
        ExtensionAction::Inspect { name } if is_builtin_system(name) => {
            print_builtin_system(config, name)?;
            Ok(Some(()))
        }
        ExtensionAction::Enable { name } if is_builtin_system(name) => {
            set_builtin_system_enabled(config, name, true)?;
            Ok(Some(()))
        }
        ExtensionAction::Disable { name } if is_builtin_system(name) => {
            set_builtin_system_enabled(config, name, false)?;
            Ok(Some(()))
        }
        ExtensionAction::Remove { name } if is_builtin_system(name) => {
            set_builtin_system_enabled(config, name, false)?;
            println!(
                "Disabled built-in system extension '{}'. Use `rove system enable {}` to restore it.",
                name, name
            );
            Ok(Some(()))
        }
        ExtensionAction::Install {
            source,
            registry,
            version,
        } if is_builtin_system(source) && registry.is_none() && version.is_none() => {
            set_builtin_system_enabled(config, source, true)?;
            println!("Installed official system extension '{}'.", source);
            Ok(Some(()))
        }
        ExtensionAction::Upgrade {
            source,
            registry,
            version,
        } if is_builtin_system(source) && registry.is_none() && version.is_none() => {
            println!(
                "Official system extension '{}' is built into this daemon build. Nothing to upgrade.",
                source
            );
            Ok(Some(()))
        }
        _ => Ok(None),
    }
}

fn print_builtin_systems(config: &Config) {
    println!("Official system extensions:");
    for (name, enabled, description) in builtin_systems(config) {
        println!(
            "- {} [{}] {}",
            name,
            if enabled { "enabled" } else { "disabled" },
            description
        );
    }
}

fn print_builtin_system(config: &Config, name: &str) -> Result<()> {
    let (enabled, description) = builtin_system_status(config, name)?;
    println!("name: {}", name);
    println!("kind: system");
    println!("source: official");
    println!("enabled: {}", enabled);
    println!("description: {}", description);
    Ok(())
}

fn set_builtin_system_enabled(config: &Config, name: &str, enabled: bool) -> Result<()> {
    let mut config = config.clone();
    match name {
        "filesystem" => config.plugins.fs_editor = enabled,
        "terminal" => config.plugins.terminal = enabled,
        "vision" => config.plugins.screenshot = enabled,
        _ => bail!("Unknown built-in system extension '{}'", name),
    }
    config.save()?;
    println!(
        "{} system '{}'.",
        if enabled { "Enabled" } else { "Disabled" },
        name
    );
    Ok(())
}

fn builtin_system_status(config: &Config, name: &str) -> Result<(bool, &'static str)> {
    match name {
        "filesystem" => Ok((
            config.plugins.fs_editor,
            "Workspace file read/write/list primitives.",
        )),
        "terminal" => Ok((
            config.plugins.terminal,
            "Secure local terminal command execution.",
        )),
        "vision" => Ok((config.plugins.screenshot, "Local screenshot capture primitives.")),
        _ => bail!("Unknown built-in system extension '{}'", name),
    }
}

fn builtin_systems(config: &Config) -> [(&'static str, bool, &'static str); 3] {
    [
        (
            "filesystem",
            config.plugins.fs_editor,
            "Workspace file read/write/list primitives.",
        ),
        (
            "terminal",
            config.plugins.terminal,
            "Secure local terminal command execution.",
        ),
        (
            "vision",
            config.plugins.screenshot,
            "Local screenshot capture primitives.",
        ),
    ]
}

fn is_builtin_system(name: &str) -> bool {
    matches!(name, "filesystem" | "terminal" | "vision")
}
