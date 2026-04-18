use std::fs;

use anyhow::Result;
use tokio::time::{sleep, Duration};

use crate::cli::commands::LogsAction;
use crate::cli::database_path::database_path;
use crate::config::Config;
use crate::storage::{AgentActionQuery, Database};
use crate::system::logs;

pub async fn handle_logs(action: LogsAction) -> Result<()> {
    match action {
        LogsAction::Tail { lines } => {
            for line in logs::recent_lines(lines)? {
                println!("{}", line);
            }
            Ok(())
        }
        LogsAction::Follow { lines } => follow_logs(lines).await,
        LogsAction::Security {
            action,
            source,
            severity,
            since_hours,
            limit,
        } => {
            let config = Config::load_or_create()?;
            let database = Database::new(&database_path(&config)).await?;
            let date_from = since_hours.map(|hours| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                now.saturating_sub(hours.max(0) * 3600)
            });
            let entries = database
                .tasks()
                .list_agent_actions(&AgentActionQuery {
                    action_type: action,
                    source,
                    severity,
                    date_from,
                    date_to: None,
                    limit,
                    offset: 0,
                })
                .await?;
            if entries.is_empty() {
                println!("No security audit log entries matched the query.");
                return Ok(());
            }
            for entry in entries {
                println!(
                    "{} [{}] action={} tool={} source={} approved_by={} task={}",
                    entry.timestamp,
                    entry.severity,
                    entry.action_type,
                    entry.tool_name,
                    entry.source.as_deref().unwrap_or("unknown"),
                    entry.approved_by,
                    entry.task_id
                );
                println!("  {}", entry.result_summary);
            }
            Ok(())
        }
    }
}

async fn follow_logs(lines: usize) -> Result<()> {
    let path = logs::log_file_path();
    println!("Following {}", path.display());

    let mut previous = read_all_lines().unwrap_or_default();
    let start = previous.len().saturating_sub(lines);
    for line in &previous[start..] {
        println!("{}", line);
    }

    loop {
        sleep(Duration::from_secs(1)).await;
        let current = read_all_lines().unwrap_or_default();
        if current.len() < previous.len() {
            previous.clear();
        }
        for line in current.iter().skip(previous.len()) {
            println!("{}", line);
        }
        previous = current;
    }
}

fn read_all_lines() -> Result<Vec<String>> {
    let path = logs::log_file_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    Ok(fs::read_to_string(path)?
        .lines()
        .map(ToOwned::to_owned)
        .collect())
}
