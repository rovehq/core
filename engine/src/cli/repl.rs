use anyhow::Result;
use tokio::io::AsyncBufReadExt;

use crate::config::metadata::{APP_DISPLAY_NAME, VERSION};
use crate::config::Config;
use crate::remote::{RemoteManager, RemoteSendOptions};
use crate::storage::pending_tasks::PendingTaskStatus;
use crate::storage::Database;
use crate::targeting::extract_task_target;
use crate::zerotier::ZeroTierManager;

use super::bootstrap;
use super::status;

const BLUE: &str = "\x1b[38;5;39m";
const GREEN: &str = "\x1b[38;5;48m";
const DIM: &str = "\x1b[38;5;240m";
const RESET: &str = "\x1b[0m";

pub async fn run() -> Result<()> {
    println!();
    println!("{BLUE}{} v{}{RESET}", APP_DISPLAY_NAME, VERSION);
    println!("  {DIM}Type a task, or /help for commands. Ctrl+C to exit.{RESET}");
    println!();
    println!("  {DIM}Starting local daemon session...{RESET}");

    let (_agent, database, gateway) = bootstrap::init_daemon().await?;
    gateway.clone().start();

    println!("  {GREEN}Daemon ready{RESET}");
    println!("  {DIM}Commands: /help, /status, /history, /quit{RESET}");
    println!();

    let stdin = tokio::io::stdin();
    let reader = tokio::io::BufReader::new(stdin);
    let mut lines = reader.lines();

    loop {
        eprint!("  \x1b[38;5;39mrove>\x1b[0m ");

        let Some(line) = lines.next_line().await? else {
            break;
        };
        let input = line.trim().to_string();
        if input.is_empty() {
            continue;
        }

        match input.as_str() {
            "/quit" | "/exit" | "/q" => break,
            "/help" | "/h" => print_help(),
            "/status" => status::show().await?,
            "/history" => print_history(&database).await?,
            "quit" | "exit" => break,
            _ => run_task(&database, &gateway, &input).await?,
        }
    }

    println!();
    println!("  Goodbye.");
    println!();
    Ok(())
}

fn print_help() {
    println!();
    println!("  Commands:");
    println!("    /status    Show system status");
    println!("    /history   Show recent task history");
    println!("    /help      Show this help");
    println!("    /quit      Exit interactive mode");
    println!();
    println!("  Or just type a task and press Enter.");
    println!();
}

async fn run_task(
    database: &std::sync::Arc<crate::storage::Database>,
    gateway: &std::sync::Arc<crate::api::gateway::Gateway>,
    input: &str,
) -> Result<()> {
    let (task, node) = extract_task_target(input);
    if let Some(node) = node {
        let config = Config::load_or_create()?;
        let manager = RemoteManager::new(config.clone());
        let mut result = manager
            .send_with_options(
                &task,
                RemoteSendOptions {
                    node: Some(node),
                    ..RemoteSendOptions::default()
                },
            )
            .await;
        if result.is_err() && config.remote.transports.zerotier.enabled {
            let _ = ZeroTierManager::new(config.clone()).refresh().await;
            result = manager
                .send_with_options(
                    &task,
                    RemoteSendOptions {
                        node: extract_task_target(input).1,
                        ..RemoteSendOptions::default()
                    },
                )
                .await;
        }

        match result {
            Ok(result) => {
                println!("  {GREEN}Done{RESET}");
                if let Some(answer) = result.answer.or(result.message) {
                    println!();
                    for line in answer.lines() {
                        println!("  {}", line);
                    }
                }
                println!();
            }
            Err(error) => {
                eprintln!("  Error: {}", error);
            }
        }
        return Ok(());
    }

    println!("  {DIM}Working...{RESET}");
    let task_id = match gateway.submit_cli(&task, None).await {
        Ok(task_id) => task_id,
        Err(error) => {
            eprintln!("  Failed to submit task: {}", error);
            return Ok(());
        }
    };

    let repository = database.pending_tasks();
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        if let Ok(Some(task)) = repository.get_task(&task_id).await {
            match task.status {
                PendingTaskStatus::Done => {
                    print_task_answer(database, &task_id).await?;
                    break;
                }
                PendingTaskStatus::Failed => {
                    eprintln!("  Error: {}", task.error.unwrap_or_default());
                    break;
                }
                _ => {}
            }
        }
    }

    Ok(())
}

async fn print_history(database: &std::sync::Arc<Database>) -> Result<()> {
    let tasks = database.tasks().get_recent_tasks(8).await?;
    println!();
    if tasks.is_empty() {
        println!("  {DIM}No task history yet.{RESET}");
        println!();
        return Ok(());
    }

    println!("  Recent tasks");
    for task in tasks {
        let status = match task.status {
            crate::storage::TaskStatus::Pending => "pending",
            crate::storage::TaskStatus::Running => "running",
            crate::storage::TaskStatus::Completed => "done",
            crate::storage::TaskStatus::Failed => "failed",
        };
        println!("  - [{}] {}", status, single_line(&task.input, 72));
    }
    println!();
    Ok(())
}

async fn print_task_answer(database: &std::sync::Arc<Database>, task_id: &str) -> Result<()> {
    let answer = database.tasks().get_latest_answer(task_id).await?;
    println!("  {GREEN}Done{RESET}");
    if let Some(answer) = answer {
        println!();
        for line in answer.lines() {
            println!("  {}", line);
        }
    }
    println!();
    Ok(())
}

fn single_line(value: &str, max_len: usize) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.len() <= max_len {
        compact
    } else {
        format!("{}...", &compact[..max_len.saturating_sub(3)])
    }
}
