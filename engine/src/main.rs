use anyhow::Result;
use clap::Parser;
use std::fs::OpenOptions;
use std::path::PathBuf;
use tracing::{error, Level};
use tracing_subscriber::{
    filter::LevelFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer,
};

use rove_engine::cli::{
    ActivateTarget, AddTarget, Cli, Command, ConfigAction, ExtensionAction,
    ExtensionFacadeAction, ExtensionKindArg, McpAction, ModelAction, OutputFormat, PluginAction,
    PolicyAction, RemoteAction, RemoteNodeAction, RemoteProfileAction, SecretsAction,
    ServiceAction, SteeringAction,
};
use rove_engine::policy::{active_workspace_policy_dir, legacy_policy_workspace_dir, policy_workspace_dir};
use rove_engine::server;
use rove_engine::policy::PolicyEngine;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Some(path) = cli.config.as_ref() {
        std::env::set_var("ROVE_CONFIG_PATH", path);
    }
    let logging_service_enabled = logging_service_enabled();
    init_logging(
        cli.verbose,
        console_log_level(&cli),
        should_honor_console_env_filter(&cli),
        logging_service_enabled,
    )?;

    match cli.command {
        None => rove_engine::cli::repl::run().await?,
        Some(Command::Start { port }) => rove_engine::cli::daemon::start_background(port)?,
        Some(Command::Stop) => rove_engine::cli::daemon::stop()?,
        Some(Command::Task {
            prompt,
            yes,
            stream,
            parallel,
            isolate,
            view,
        }) => {
            let config = rove_engine::config::Config::load_or_create()?;
            rove_engine::cli::run::handle_run(
                rove_engine::cli::run::RunRequest {
                    task: prompt.join(" "),
                    auto_approve: yes,
                    stream,
                    parallel,
                    isolate,
                    view,
                    format: OutputFormat::Text,
                },
                &config,
            )
            .await?;
        }
        Some(Command::History { limit }) => {
            let config = rove_engine::config::Config::load_or_create()?;
            rove_engine::cli::history::handle_history(limit, &config, OutputFormat::Text).await?;
        }
        Some(Command::Replay { task_id }) => {
            let config = rove_engine::config::Config::load_or_create()?;
            rove_engine::cli::replay::handle_replay(task_id, &config, OutputFormat::Text).await?;
        }
        Some(Command::Status) => rove_engine::cli::status::show()?,
        Some(Command::Unlock) => rove_engine::cli::unlock::run().await?,
        Some(Command::Plugin { action }) => {
            eprintln!(
                "Compatibility alias: `rove plugin` remains available, but prefer `rove skill`, `rove system`, `rove channel`, or `rove connector`."
            );
            handle_plugin(action).await?
        }
        Some(Command::Steer { action, dir }) => {
            eprintln!("Compatibility alias: `rove steer` remains available, but prefer `rove policy`.");
            handle_steering(action, dir).await?
        }
        Some(Command::Policy { action, dir }) => {
            let config = rove_engine::config::Config::load_or_create()?;
            handle_policy(action, dir, &config).await?;
        }
        Some(Command::Extension { action }) => {
            let config = rove_engine::config::Config::load_or_create()?;
            handle_extension_facade(action, &config).await?;
        }
        Some(Command::Skill { action }) => {
            let config = rove_engine::config::Config::load_or_create()?;
            handle_extension(action, &config, rove_engine::cli::extensions::ExtensionSurface::Skill)
                .await?;
        }
        Some(Command::System { action }) => {
            let config = rove_engine::config::Config::load_or_create()?;
            handle_extension(action, &config, rove_engine::cli::extensions::ExtensionSurface::System)
                .await?;
        }
        Some(Command::Connector { action }) => {
            let config = rove_engine::config::Config::load_or_create()?;
            handle_mcp(action, &config).await?;
        }
        Some(Command::Channel { action }) => {
            let config = rove_engine::config::Config::load_or_create()?;
            handle_extension(action, &config, rove_engine::cli::extensions::ExtensionSurface::Channel)
                .await?;
        }
        Some(Command::Service { action }) => handle_service(action).await?,
        Some(Command::Remote { action }) => {
            let config = rove_engine::config::Config::load_or_create()?;
            handle_remote(action, &config).await?;
        }
        Some(Command::Add { target }) => handle_add(target).await?,
        Some(Command::Activate { target }) => handle_activate(target, true).await?,
        Some(Command::Deactivate { target }) => handle_activate(target, false).await?,
        Some(Command::Model { action }) => handle_model(action).await?,
        Some(Command::Schedule { action }) => {
            let config = rove_engine::config::Config::load_or_create()?;
            rove_engine::cli::schedule::handle_schedule(action, &config).await?;
        }
        Some(Command::Config { action }) => {
            let config = rove_engine::config::Config::load_or_create()?;
            handle_config(action, &config).await?;
        }
        Some(Command::Secrets { action }) => handle_secrets(action).await?,
        Some(Command::Mcp { action }) => {
            eprintln!(
                "Compatibility alias: `rove mcp` remains available, but prefer `rove connector`."
            );
            let config = rove_engine::config::Config::load_or_create()?;
            handle_mcp(action, &config).await?;
        }
        Some(Command::Brain { action }) => rove_engine::cli::brain::execute(action).await?,
        Some(Command::Daemon { port }) => run_daemon(port).await?,
        Some(Command::Doctor) => {
            let config = rove_engine::config::Config::load_or_create()?;
            rove_engine::cli::doctor::handle_doctor(&config, OutputFormat::Text).await?;
        }
        Some(Command::Keys) => println!("Use: python3 scripts/generate_keys.py"),
        Some(Command::Update { check }) => {
            rove_engine::cli::update::handle_update(check, OutputFormat::Text).await?;
        }
        Some(Command::Setup) => rove_engine::cli::setup::handle_setup().await?,
    }

    Ok(())
}

fn init_logging(
    verbose: bool,
    console_level: LevelFilter,
    honor_console_env_filter: bool,
    logging_service_enabled: bool,
) -> Result<()> {
    if !logging_service_enabled && !verbose && !honor_console_env_filter {
        return tracing_subscriber::registry()
            .try_init()
            .map_err(|error| anyhow::anyhow!("setting default subscriber failed: {}", error));
    }

    let level = if verbose { Level::DEBUG } else { Level::INFO };
    let default_filter = EnvFilter::new(format!("rove_engine={}", level.as_str().to_lowercase()));
    let console_layer = fmt::layer().with_target(false);

    let log_path = log_file_path();
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file_path = log_path.clone();
    let file_layer = fmt::layer().with_ansi(false).with_writer(move || {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .expect("failed to open log file")
    });

    let console_layer = if honor_console_env_filter {
        if let Ok(env_filter) = EnvFilter::try_from_default_env() {
            console_layer.with_filter(env_filter).boxed()
        } else {
            console_layer.with_filter(console_level).boxed()
        }
    } else {
        console_layer.with_filter(console_level).boxed()
    };

    let file_filter = EnvFilter::try_from_default_env().unwrap_or(default_filter);
    let file_layer = file_layer.with_filter(file_filter).boxed();

    tracing_subscriber::registry()
        .with(console_layer)
        .with(file_layer)
        .try_init()
        .map_err(|error| anyhow::anyhow!("setting default subscriber failed: {}", error))
}

fn logging_service_enabled() -> bool {
    let Ok(config_path) = rove_engine::config::Config::config_path() else {
        return true;
    };
    if !config_path.exists() {
        return true;
    }
    match rove_engine::config::Config::load_from_path(&config_path) {
        Ok(config) => !config.core.log_level.eq_ignore_ascii_case("error"),
        Err(_) => true,
    }
}

fn console_log_level(cli: &Cli) -> LevelFilter {
    if cli.verbose {
        return LevelFilter::DEBUG;
    }

    match &cli.command {
        None => LevelFilter::ERROR,
        Some(Command::Task {
            view: rove_engine::cli::TaskView::Logs,
            ..
        }) => LevelFilter::INFO,
        Some(Command::Task { .. }) => LevelFilter::ERROR,
        _ => LevelFilter::INFO,
    }
}

fn should_honor_console_env_filter(cli: &Cli) -> bool {
    if cli.verbose {
        return true;
    }

    matches!(
        &cli.command,
        Some(Command::Task {
            view: rove_engine::cli::TaskView::Logs,
            ..
        })
    )
}

fn log_file_path() -> PathBuf {
    if let Some(data_dir) = std::env::var_os("ROVE_DATA_DIR").filter(|value| !value.is_empty()) {
        let data_dir = PathBuf::from(data_dir);
        if let Some(parent) = data_dir.parent() {
            return parent.join("rove.log");
        }
        return data_dir.join("rove.log");
    }

    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".rove")
        .join("rove.log")
}

async fn run_daemon(port: u16) -> Result<()> {
    let config = rove_engine::config::Config::load_or_create()?;
    // Runtime manager bootstrap happens inside CLI bootstrap:
    // builtins are registered immediately, while plugin schemas are loaded
    // without starting WASM modules or MCP servers until first use.
    let (agent, database, gateway) = rove_engine::cli::bootstrap::init_daemon().await?;
    gateway.clone().start();
    rove_engine::channels::manager::ChannelManager::new(config.clone())
        .start_enabled(gateway.clone(), database.clone());
    tracing::info!("{}", rove_engine::info::engine_banner());
    server::start_daemon(agent, port, database, gateway, config.webui.enabled).await?;
    Ok(())
}

async fn handle_steering(action: SteeringAction, dir: Option<std::path::PathBuf>) -> Result<()> {
    let config = rove_engine::config::Config::load_or_create()?;
    let policy_dir = dir.unwrap_or_else(|| config.policy.policy_dir().clone());
    let cwd = std::env::current_dir().unwrap_or_else(|_| config.core.workspace.clone());
    let primary_workspace_dir = policy_workspace_dir(&cwd);
    let legacy_workspace_dir = legacy_policy_workspace_dir(&cwd);
    let workspace_dir = active_workspace_policy_dir(&primary_workspace_dir, &legacy_workspace_dir);
    let engine = PolicyEngine::new_with_workspace(&policy_dir, Some(&workspace_dir)).await?;

    match action {
        SteeringAction::List => {
            let mut all = engine.list_policies().await;
            all.sort_by(|a, b| a.file_path.cmp(&b.file_path));
            println!("{} policy file(s) loaded", all.len());
            for policy in all {
                let domains = policy
                    .config
                    .as_ref()
                    .map(|cfg| {
                        if cfg.meta.domains.is_empty() {
                            "-".to_string()
                        } else {
                            cfg.meta.domains.join(",")
                        }
                    })
                    .unwrap_or_else(|| "-".to_string());
                println!("- {} [{}] {}", policy.id, domains, policy.file_path.display());
            }
        }
        SteeringAction::On { name } => {
            if let Err(error) = engine.activate_policy(&name).await {
                error!("{}", error);
            } else {
                println!("Activated '{}'", name);
            }
        }
        SteeringAction::Off { name } => {
            engine.deactivate_policy(&name).await;
            println!("Deactivated '{}'", name);
        }
        SteeringAction::Status => {
            let domain = infer_steering_domain(&cwd);
            engine.auto_activate_policies("", 0, Some(domain)).await;
            let active = engine.active_policies().await;
            let directives = engine.get_directives().await;
            println!("Active policy directives for domain '{}':", domain);
            if active.is_empty() {
                println!("(none)");
            } else {
                for policy in active {
                    println!("- {}", policy);
                }
            }
            if !directives.system_prefix.is_empty() {
                println!();
                println!("{}", directives.system_prefix);
            }
            if !directives.system_suffix.is_empty() {
                println!();
                println!("{}", directives.system_suffix);
            }
        }
        SteeringAction::Default => {
            rove_engine::policy::bootstrap_builtins(&policy_dir).await?;
            println!(
                "Built-in policy files ready in {}",
                policy_dir.display()
            );
        }
    }

    Ok(())
}

fn infer_steering_domain(cwd: &std::path::Path) -> &'static str {
    if cwd.join("Cargo.toml").exists() || cwd.join("src").exists() {
        return "code";
    }
    if cwd.join(".git").exists() {
        return "git";
    }
    "general"
}

async fn handle_plugin(action: PluginAction) -> Result<()> {
    let config = rove_engine::config::Config::load_or_create()?;
    match action {
        PluginAction::New { name, plugin_type } => {
            rove_engine::cli::plugins::handle_new(&name, plugin_type).await?;
        }
        PluginAction::Test {
            source,
            tool,
            input,
            files,
            args,
            no_build,
        } => {
            rove_engine::cli::plugins::handle_test(
                source.as_deref(),
                tool.as_deref(),
                input.as_deref(),
                &files,
                &args,
                no_build,
            )
            .await?;
        }
        PluginAction::Pack {
            source,
            out,
            no_build,
        } => {
            rove_engine::cli::plugins::handle_pack(source.as_deref(), out.as_deref(), no_build)
                .await?;
        }
        PluginAction::Publish {
            source,
            registry_dir,
            no_build,
        } => {
            rove_engine::cli::plugins::handle_publish(source.as_deref(), &registry_dir, no_build)
                .await?;
        }
        PluginAction::List => {
            rove_engine::cli::plugins::handle_list(&config, OutputFormat::Text).await?;
        }
        PluginAction::Inspect { name } => {
            rove_engine::cli::plugins::handle_inspect(&config, &name).await?;
        }
        PluginAction::Enable { name } => {
            rove_engine::cli::plugins::handle_set_enabled(&config, &name, true).await?;
        }
        PluginAction::Disable { name } => {
            rove_engine::cli::plugins::handle_set_enabled(&config, &name, false).await?;
        }
        PluginAction::Install {
            source,
            registry,
            version,
        } => {
            rove_engine::cli::plugins::handle_install(
                &config,
                &source,
                registry.as_deref(),
                version.as_deref(),
            )
            .await?;
        }
        PluginAction::Upgrade {
            source,
            registry,
            version,
        } => {
            rove_engine::cli::plugins::handle_upgrade(
                &config,
                &source,
                registry.as_deref(),
                version.as_deref(),
            )
            .await?;
        }
        PluginAction::Remove { name } => {
            rove_engine::cli::plugins::handle_remove(&config, &name).await?;
        }
    }
    Ok(())
}

async fn handle_extension(
    action: ExtensionAction,
    config: &rove_engine::config::Config,
    surface: rove_engine::cli::extensions::ExtensionSurface,
) -> Result<()> {
    rove_engine::cli::extensions::handle(config, surface, action).await
}

async fn handle_extension_facade(
    action: ExtensionFacadeAction,
    config: &rove_engine::config::Config,
) -> Result<()> {
    match action {
        ExtensionFacadeAction::New { kind, name } => {
            match kind {
                ExtensionKindArg::Skill | ExtensionKindArg::System | ExtensionKindArg::Channel => {
                    handle_extension(
                        ExtensionAction::New { name },
                        config,
                        extension_surface(kind),
                    )
                    .await
                }
                ExtensionKindArg::Connector => {
                    anyhow::bail!(
                        "Connector authoring uses the dedicated surface. Use `rove connector scaffold ...` or `rove connector add ...`."
                    )
                }
            }
        }
        ExtensionFacadeAction::Test {
            kind,
            source,
            tool,
            input,
            files,
            args,
            no_build,
        } => match kind {
            ExtensionKindArg::Skill | ExtensionKindArg::System | ExtensionKindArg::Channel => {
                handle_extension(
                    ExtensionAction::Test {
                        source,
                        tool,
                        input,
                        files,
                        args,
                        no_build,
                    },
                    config,
                    extension_surface(kind),
                )
                .await
            }
            ExtensionKindArg::Connector => {
                anyhow::bail!("Connector testing uses `rove connector test <name>`.")
            }
        },
        ExtensionFacadeAction::Pack {
            kind,
            source,
            out,
            no_build,
        } => match kind {
            ExtensionKindArg::Skill | ExtensionKindArg::System | ExtensionKindArg::Channel => {
                handle_extension(
                    ExtensionAction::Pack {
                        source,
                        out,
                        no_build,
                    },
                    config,
                    extension_surface(kind),
                )
                .await
            }
            ExtensionKindArg::Connector => {
                anyhow::bail!("Connector packaging uses `rove connector scaffold/export/install`.")
            }
        },
        ExtensionFacadeAction::Publish {
            kind,
            source,
            registry_dir,
            no_build,
        } => match kind {
            ExtensionKindArg::Skill | ExtensionKindArg::System | ExtensionKindArg::Channel => {
                handle_extension(
                    ExtensionAction::Publish {
                        source,
                        registry_dir,
                        no_build,
                    },
                    config,
                    extension_surface(kind),
                )
                .await
            }
            ExtensionKindArg::Connector => {
                anyhow::bail!("Connector publishing uses the MCP catalog flow, not `rove extension publish connector`.")
            }
        },
        ExtensionFacadeAction::Install {
            kind,
            source,
            registry,
            version,
        } => match kind {
            ExtensionKindArg::Skill | ExtensionKindArg::System | ExtensionKindArg::Channel => {
                handle_extension(
                    ExtensionAction::Install {
                        source,
                        registry,
                        version,
                    },
                    config,
                    extension_surface(kind),
                )
                .await
            }
            ExtensionKindArg::Connector => {
                handle_mcp(McpAction::Install { source }, config).await
            }
        },
        ExtensionFacadeAction::Upgrade {
            kind,
            source,
            registry,
            version,
        } => match kind {
            ExtensionKindArg::Skill | ExtensionKindArg::System | ExtensionKindArg::Channel => {
                handle_extension(
                    ExtensionAction::Upgrade {
                        source,
                        registry,
                        version,
                    },
                    config,
                    extension_surface(kind),
                )
                .await
            }
            ExtensionKindArg::Connector => {
                if registry.is_some() || version.is_some() {
                    anyhow::bail!(
                        "Connector upgrades currently accept only a local package directory: `rove connector upgrade <source>`."
                    );
                }
                handle_mcp(McpAction::Upgrade { source }, config).await
            }
        },
        ExtensionFacadeAction::List { kind } => match kind {
            Some(kind @ ExtensionKindArg::Skill)
            | Some(kind @ ExtensionKindArg::System)
            | Some(kind @ ExtensionKindArg::Channel) => {
                handle_extension(ExtensionAction::List, config, extension_surface(kind)).await
            }
            Some(ExtensionKindArg::Connector) => handle_mcp(McpAction::List, config).await,
            None => handle_plugin(PluginAction::List).await,
        },
        ExtensionFacadeAction::Inspect { kind, name } => match kind {
            ExtensionKindArg::Skill | ExtensionKindArg::System | ExtensionKindArg::Channel => {
                handle_extension(
                    ExtensionAction::Inspect { name },
                    config,
                    extension_surface(kind),
                )
                .await
            }
            ExtensionKindArg::Connector => handle_mcp(McpAction::Show { name }, config).await,
        },
        ExtensionFacadeAction::Enable { kind, name } => match kind {
            ExtensionKindArg::Skill | ExtensionKindArg::System | ExtensionKindArg::Channel => {
                handle_extension(
                    ExtensionAction::Enable { name },
                    config,
                    extension_surface(kind),
                )
                .await
            }
            ExtensionKindArg::Connector => {
                handle_mcp(McpAction::Enable { name }, config).await
            }
        },
        ExtensionFacadeAction::Disable { kind, name } => match kind {
            ExtensionKindArg::Skill | ExtensionKindArg::System | ExtensionKindArg::Channel => {
                handle_extension(
                    ExtensionAction::Disable { name },
                    config,
                    extension_surface(kind),
                )
                .await
            }
            ExtensionKindArg::Connector => {
                handle_mcp(McpAction::Disable { name }, config).await
            }
        },
        ExtensionFacadeAction::Remove { kind, name } => match kind {
            ExtensionKindArg::Skill | ExtensionKindArg::System | ExtensionKindArg::Channel => {
                handle_extension(
                    ExtensionAction::Remove { name },
                    config,
                    extension_surface(kind),
                )
                .await
            }
            ExtensionKindArg::Connector => {
                handle_mcp(McpAction::Remove { name }, config).await
            }
        },
    }
}

async fn handle_model(action: ModelAction) -> Result<()> {
    match action {
        ModelAction::Setup => rove_engine::cli::model::handle_setup().await?,
        ModelAction::List => rove_engine::cli::model::handle_list().await?,
        ModelAction::Pull { name } => {
            println!("Model pull for '{}' is not implemented yet.", name);
        }
    }
    Ok(())
}

async fn handle_config(action: ConfigAction, config: &rove_engine::config::Config) -> Result<()> {
    match action {
        ConfigAction::Show => rove_engine::cli::config::show(config),
    }
}

async fn handle_secrets(action: SecretsAction) -> Result<()> {
    match action {
        SecretsAction::Set { name } => rove_engine::cli::secrets::set(&name).await,
        SecretsAction::List => rove_engine::cli::secrets::list().await,
        SecretsAction::Remove { name } => rove_engine::cli::secrets::remove(&name).await,
    }
}

async fn handle_mcp(action: McpAction, config: &rove_engine::config::Config) -> Result<()> {
    rove_engine::cli::mcp::handle(action, config).await
}

async fn handle_policy(
    action: PolicyAction,
    dir: Option<std::path::PathBuf>,
    config: &rove_engine::config::Config,
) -> Result<()> {
    rove_engine::cli::policy::handle(action, dir, config).await
}

async fn handle_service(action: ServiceAction) -> Result<()> {
    let mut config = rove_engine::config::Config::load_or_create()?;
    match action {
        ServiceAction::List => rove_engine::cli::service::list(&config),
        ServiceAction::Show { name } => {
            rove_engine::cli::service::handle(
                rove_engine::cli::service::ServiceAction::Show,
                name,
                &mut config,
            )?;
        }
        ServiceAction::Enable { name } => {
            rove_engine::cli::service::handle(
                rove_engine::cli::service::ServiceAction::Enable,
                name,
                &mut config,
            )?;
        }
        ServiceAction::Disable { name } => {
            rove_engine::cli::service::handle(
                rove_engine::cli::service::ServiceAction::Disable,
                name,
                &mut config,
            )?;
        }
    }
    Ok(())
}

async fn handle_remote(action: RemoteAction, config: &rove_engine::config::Config) -> Result<()> {
    let action = match action {
        RemoteAction::Status => rove_engine::cli::remote::RemoteAction::Status,
        RemoteAction::Node { action } => match action {
            RemoteNodeAction::List => rove_engine::cli::remote::RemoteAction::Nodes,
            RemoteNodeAction::Rename { name } => {
                rove_engine::cli::remote::RemoteAction::Rename(name)
            }
            RemoteNodeAction::Pair {
                target,
                url,
                token,
                executor_only,
                tags,
                capabilities,
            } => rove_engine::cli::remote::RemoteAction::Pair {
                target,
                url,
                token,
                executor_only,
                tags,
                capabilities,
            },
            RemoteNodeAction::Unpair { name } => {
                rove_engine::cli::remote::RemoteAction::Unpair(name)
            }
            RemoteNodeAction::Trust { name } => {
                rove_engine::cli::remote::RemoteAction::Trust(name)
            }
        },
        RemoteAction::Profile { action } => match action {
            RemoteProfileAction::Show => rove_engine::cli::remote::RemoteAction::ProfileShow,
            RemoteProfileAction::ExecutorOnly => {
                rove_engine::cli::remote::RemoteAction::ProfileExecutorOnly
            }
            RemoteProfileAction::Full => rove_engine::cli::remote::RemoteAction::ProfileFull,
            RemoteProfileAction::Tags { tags } => {
                rove_engine::cli::remote::RemoteAction::ProfileTags(tags)
            }
            RemoteProfileAction::Capabilities { capabilities } => {
                rove_engine::cli::remote::RemoteAction::ProfileCapabilities(capabilities)
            }
        },
        RemoteAction::Nodes => rove_engine::cli::remote::RemoteAction::Nodes,
        RemoteAction::Rename { name } => rove_engine::cli::remote::RemoteAction::Rename(name),
        RemoteAction::Pair {
            target,
            url,
            token,
            executor_only,
            tags,
            capabilities,
        } => rove_engine::cli::remote::RemoteAction::Pair {
            target,
            url,
            token,
            executor_only,
            tags,
            capabilities,
        },
        RemoteAction::Unpair { name } => rove_engine::cli::remote::RemoteAction::Unpair(name),
        RemoteAction::Trust { name } => rove_engine::cli::remote::RemoteAction::Trust(name),
        RemoteAction::Send {
            node,
            tags,
            capabilities,
            allow_executor_only,
            prefer_executor_only,
            prompt,
        } => rove_engine::cli::remote::RemoteAction::Send {
            node,
            tags,
            capabilities,
            allow_executor_only,
            prefer_executor_only,
            prompt: prompt.join(" "),
        },
    };
    rove_engine::cli::remote::handle(action, config).await
}

fn extension_surface(
    kind: ExtensionKindArg,
) -> rove_engine::cli::extensions::ExtensionSurface {
    match kind {
        ExtensionKindArg::Skill => rove_engine::cli::extensions::ExtensionSurface::Skill,
        ExtensionKindArg::System => rove_engine::cli::extensions::ExtensionSurface::System,
        ExtensionKindArg::Channel => rove_engine::cli::extensions::ExtensionSurface::Channel,
        ExtensionKindArg::Connector => unreachable!("connectors use MCP handlers"),
    }
}

async fn handle_add(target: AddTarget) -> Result<()> {
    let mut config = rove_engine::config::Config::load_or_create()?;
    match target {
        AddTarget::Mcp => {
            rove_engine::cli::service::handle(
                rove_engine::cli::service::ServiceAction::Enable,
                rove_engine::cli::ServiceTarget::ConnectorEngine,
                &mut config,
            )?;
            println!("Connector support is available. Add a connector next with `rove connector add ...` or `rove connector install ...`.");
        }
    }
    Ok(())
}

async fn handle_activate(target: ActivateTarget, enabled: bool) -> Result<()> {
    let mut config = rove_engine::config::Config::load_or_create()?;
    let service = match target {
        ActivateTarget::Logging => rove_engine::cli::ServiceTarget::Logging,
        ActivateTarget::Webui => rove_engine::cli::ServiceTarget::Webui,
        ActivateTarget::Remote => rove_engine::cli::ServiceTarget::Remote,
    };
    rove_engine::cli::service::handle(
        if enabled {
            rove_engine::cli::service::ServiceAction::Enable
        } else {
            rove_engine::cli::service::ServiceAction::Disable
        },
        service,
        &mut config,
    )?;
    Ok(())
}
