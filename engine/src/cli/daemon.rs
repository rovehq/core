use std::path::PathBuf;

use anyhow::Result;

use crate::config::metadata::{APP_DISPLAY_NAME, VERSION};

pub fn start_background(port: u16) -> Result<()> {
    let executable = std::env::current_exe()?;
    let child = std::process::Command::new(&executable)
        .args(["daemon", "--port", &port.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()?;

    let pid_file = pid_file();
    if let Some(parent) = pid_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&pid_file, child.id().to_string())?;

    println!();
    println!("  {} v{}", APP_DISPLAY_NAME, VERSION);
    println!("  Daemon started (PID {}, port {})", child.id(), port);
    println!();
    Ok(())
}

pub fn stop() -> Result<()> {
    let pid_file = pid_file();
    if !pid_file.exists() {
        println!("  No daemon running");
        return Ok(());
    }

    let pid_str = std::fs::read_to_string(&pid_file)?;
    if let Ok(pid) = pid_str.trim().parse::<i32>() {
        #[cfg(unix)]
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/F"])
                .output();
        }
        println!("  Stopped daemon (PID {})", pid);
    }

    let _ = std::fs::remove_file(pid_file);
    Ok(())
}

pub fn is_running() -> bool {
    let pid_file = pid_file();
    if !pid_file.exists() {
        return false;
    }

    let Ok(pid_str) = std::fs::read_to_string(&pid_file) else {
        return false;
    };
    let Ok(pid) = pid_str.trim().parse::<i32>() else {
        return false;
    };

    #[cfg(unix)]
    {
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[cfg(not(unix))]
    {
        true
    }
}

pub fn pid_file() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".rove")
        .join("rove.pid")
}
