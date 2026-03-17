use anyhow::Result;

use crate::config::metadata::{APP_DISPLAY_NAME, VERSION};
use crate::config::Config;

use super::daemon;

pub fn show() -> Result<()> {
    let _config = Config::load_or_create()?;
    let running = daemon::is_running()?;
    let pid_file = daemon::pid_file()?;

    println!();
    println!("  {} v{}", APP_DISPLAY_NAME, VERSION);
    println!("  Daemon: {}", if running { "running" } else { "stopped" });

    if running {
        if let Ok(pid) = std::fs::read_to_string(pid_file) {
            println!("  PID:    {}", pid.trim());
        }
    }

    println!();
    Ok(())
}
