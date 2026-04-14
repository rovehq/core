use std::fs;

use anyhow::Result;
use tokio::time::{sleep, Duration};

use crate::cli::commands::LogsAction;
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
