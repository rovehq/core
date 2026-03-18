use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;

use crate::cli::database_path::expand_data_dir;
use crate::config::metadata::{APP_DISPLAY_NAME, VERSION};
use crate::config::Config;

pub fn start_background(port: u16) -> Result<()> {
    let config = Config::load_or_create()?;
    let executable = std::env::current_exe()?;
    let mut command = std::process::Command::new(&executable);
    command
        .args(["daemon", "--port", &port.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        // Detach from the caller's session so the daemon survives `rove start`.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    let mut child = command.spawn()?;

    let pid_file = pid_file_path(&config);
    if let Some(parent) = pid_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&pid_file, child.id().to_string())?;
    std::thread::sleep(Duration::from_millis(250));
    if let Some(status) = child.try_wait()? {
        let _ = std::fs::remove_file(&pid_file);
        anyhow::bail!("daemon exited immediately with status {}", status);
    }

    println!();
    println!("  {} v{}", APP_DISPLAY_NAME, VERSION);
    println!("  Daemon started (PID {}, port {})", child.id(), port);
    println!();
    Ok(())
}

pub fn stop() -> Result<()> {
    let config = Config::load_or_create()?;
    let pid_file = pid_file_path(&config);
    if !pid_file.exists() {
        let _ = crate::cli::brain::stop_local_server();
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
    let _ = crate::cli::brain::stop_local_server();
    Ok(())
}

pub fn is_running() -> Result<bool> {
    let config = Config::load_or_create()?;
    let pid_file = pid_file_path(&config);
    if !pid_file.exists() {
        return Ok(false);
    }

    let Ok(pid_str) = std::fs::read_to_string(&pid_file) else {
        return Ok(false);
    };
    let Ok(pid) = pid_str.trim().parse::<i32>() else {
        return Ok(false);
    };

    #[cfg(unix)]
    {
        Ok(unsafe { libc::kill(pid, 0) == 0 })
    }

    #[cfg(not(unix))]
    {
        Ok(true)
    }
}

pub fn pid_file() -> Result<PathBuf> {
    let config = Config::load_or_create()?;
    Ok(pid_file_path(&config))
}

fn pid_file_path(config: &Config) -> PathBuf {
    expand_data_dir(&config.core.data_dir).join("rove.pid")
}
