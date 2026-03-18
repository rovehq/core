use anyhow::Result;
use clap::Parser;
use tracing::{error, Level};
use tracing_subscriber::FmtSubscriber;

use rove_engine::cli::{Cli, Command, ModelAction, OutputFormat, PluginAction, SteeringAction};
use rove_engine::server;
use rove_engine::steering::loader::SteeringEngine;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    init_logging(cli.verbose)?;

    match cli.command {
        None => rove_engine::cli::repl::run().await?,
        Some(Command::Start { port }) => rove_engine::cli::daemon::start_background(port)?,
        Some(Command::Stop) => rove_engine::cli::daemon::stop()?,
        Some(Command::Task { prompt, yes }) => {
            let config = rove_engine::config::Config::load_or_create()?;
            rove_engine::cli::run::handle_run(prompt.join(" "), yes, &config, OutputFormat::Text)
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
    let subscriber = FmtSubscriber::builder().with_max_level(level).finish();
    tracing::subscriber::set_global_default(subscriber)
        .map_err(|error| anyhow::anyhow!("setting default subscriber failed: {}", error))
}

async fn run_daemon(port: u16) -> Result<()> {
    let (agent, database, gateway) = rove_engine::cli::bootstrap::init_daemon().await?;
    gateway.clone().start(agent.clone());
    tracing::info!("{}", rove_engine::info::engine_banner());
    server::start_daemon(agent, port, database, gateway).await?;
    Ok(())
}

async fn handle_steering(action: SteeringAction, dir: Option<std::path::PathBuf>) -> Result<()> {
    let home_dir =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    let steering_dir = dir.unwrap_or_else(|| home_dir.join(".rove").join("steering"));
    let engine = SteeringEngine::new(&steering_dir).await?;

    match action {
        SteeringAction::List => {
            let all = engine.list_skills().await;
            println!("{} steering file(s) loaded", all.len());
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
            let active = engine.active_skills().await;
            println!("{} steering file(s) active", active.len());
        }
        SteeringAction::Default => println!("Built-in steering files confirmed"),
    }

    Ok(())
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
