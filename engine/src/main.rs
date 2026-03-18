use anyhow::Result;
use clap::Parser;
use std::fs::OpenOptions;
use std::path::PathBuf;
use tracing::{error, Level};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use rove_engine::channels::TelegramBot;
use rove_engine::cli::{
    Cli, Command, ConfigAction, ModelAction, OutputFormat, PluginAction, SecretsAction,
    SteeringAction,
};
use rove_engine::config::metadata::SERVICE_NAME;
use rove_engine::security::secrets::SecretManager;
use rove_engine::server;
use rove_engine::steering::loader::SteeringEngine;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Some(path) = cli.config.as_ref() {
        std::env::set_var("ROVE_CONFIG_PATH", path);
    }
    init_logging(cli.verbose)?;

    match cli.command {
        None => rove_engine::cli::repl::run().await?,
        Some(Command::Start { port }) => rove_engine::cli::daemon::start_background(port)?,
        Some(Command::Stop) => rove_engine::cli::daemon::stop()?,
        Some(Command::Task {
            prompt,
            yes,
            stream,
        }) => {
            let config = rove_engine::config::Config::load_or_create()?;
            rove_engine::cli::run::handle_run(
                prompt.join(" "),
                yes,
                stream,
                &config,
                OutputFormat::Text,
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
        Some(Command::Plugin { action }) => handle_plugin(action).await?,
        Some(Command::Steer { action, dir }) => handle_steering(action, dir).await?,
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

fn init_logging(verbose: bool) -> Result<()> {
    let level = if verbose { Level::DEBUG } else { Level::INFO };
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(format!("rove_engine={}", level.as_str().to_lowercase()))
    });

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

    tracing_subscriber::registry()
        .with(env_filter)
        .with(console_layer)
        .with(file_layer)
        .try_init()
        .map_err(|error| anyhow::anyhow!("setting default subscriber failed: {}", error))
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
    let (agent, database, gateway) = rove_engine::cli::bootstrap::init_daemon().await?;
    gateway.clone().start();
    start_telegram_if_enabled(&config, gateway.clone(), database.clone());
    tracing::info!("{}", rove_engine::info::engine_banner());
    server::start_daemon(agent, port, database, gateway).await?;
    Ok(())
}

async fn handle_steering(action: SteeringAction, dir: Option<std::path::PathBuf>) -> Result<()> {
    let config = rove_engine::config::Config::load_or_create()?;
    let steering_dir = dir.unwrap_or_else(|| config.steering.skill_dir.clone());
    let cwd = std::env::current_dir().unwrap_or_else(|_| config.core.workspace.clone());
    let workspace_dir = cwd.join(".rove").join("steering");
    let engine = SteeringEngine::new_with_workspace(&steering_dir, Some(&workspace_dir)).await?;

    match action {
        SteeringAction::List => {
            let mut all = engine.list_skills().await;
            all.sort_by(|a, b| a.file_path.cmp(&b.file_path));
            println!("{} steering file(s) loaded", all.len());
            for skill in all {
                let domains = skill
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
                println!("- {} [{}] {}", skill.id, domains, skill.file_path.display());
            }
        }
        SteeringAction::On { name } => {
            if let Err(error) = engine.activate(&name).await {
                error!("{}", error);
            } else {
                println!("Activated '{}'", name);
            }
        }
        SteeringAction::Off { name } => {
            engine.deactivate(&name).await;
            println!("Deactivated '{}'", name);
        }
        SteeringAction::Status => {
            let domain = infer_steering_domain(&cwd);
            engine.auto_activate("", 0, Some(domain)).await;
            let active = engine.active_skills().await;
            let directives = engine.get_directives().await;
            println!("Active steering for domain '{}':", domain);
            if active.is_empty() {
                println!("(none)");
            } else {
                for skill in active {
                    println!("- {}", skill);
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
            rove_engine::steering::bootstrap_builtins(&steering_dir).await?;
            println!(
                "Built-in steering files ready in {}",
                steering_dir.display()
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
        PluginAction::List => {
            rove_engine::cli::plugins::handle_list(&config, OutputFormat::Text).await?;
        }
        PluginAction::Install { name } => {
            println!("Plugin install for '{}' is not implemented yet.", name);
        }
        PluginAction::Remove { name } => {
            println!("Plugin removal for '{}' is not implemented yet.", name);
        }
    }
    Ok(())
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

fn start_telegram_if_enabled(
    config: &rove_engine::config::Config,
    gateway: std::sync::Arc<rove_engine::gateway::Gateway>,
    database: std::sync::Arc<rove_engine::db::Database>,
) {
    if !config.telegram.enabled {
        return;
    }

    let config = config.clone();
    tokio::spawn(async move {
        let secret_manager = SecretManager::new(SERVICE_NAME);
        if !secret_manager.has_secret("telegram_token").await {
            tracing::warn!(
                "Telegram is enabled but no telegram_token is configured. Run `rove secrets set telegram`."
            );
            return;
        }

        let token = match secret_manager.get_secret("telegram_token").await {
            Ok(token) => token,
            Err(error) => {
                tracing::warn!("Failed to load telegram token: {}", error);
                return;
            }
        };

        let mut bot = TelegramBot::new(token, config.telegram.allowed_ids.clone())
            .with_gateway(gateway, database);
        if let Some(chat_id) = config.telegram.confirmation_chat_id {
            bot = bot.with_confirmation_chat(chat_id);
        }
        if let Some(base_url) = config.telegram.api_base_url {
            bot = bot.with_api_base_url(base_url);
        }

        if let Err(error) = bot.start_polling().await {
            tracing::error!("Telegram polling stopped: {}", error);
        }
    });
}
