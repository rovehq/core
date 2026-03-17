use anyhow::Result;
use tokio::io::AsyncBufReadExt;

use crate::config::metadata::{APP_DISPLAY_NAME, VERSION};
use crate::storage::pending_tasks::PendingTaskStatus;

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
    println!("  {DIM}Initializing daemon (gateway + agent)...{RESET}");

    let (agent, database, gateway) = bootstrap::init_daemon().await?;
    gateway.clone().start(agent.clone());

    println!("  {GREEN}Gateway poll loop started{RESET}");
    println!("  {GREEN}Agent ready{RESET}");
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
            "/status" => status::show()?,
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
    println!("  Processing...");
    let task_id = match gateway.submit_cli(input, None).await {
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
                    println!("  {GREEN}Task completed{RESET}");
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
