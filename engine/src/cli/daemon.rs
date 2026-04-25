use std::path::PathBuf;
use std::time::Duration;
use std::{fs::OpenOptions, process::Stdio};

use anyhow::Result;

use crate::config::metadata::{APP_DISPLAY_NAME, VERSION};
use crate::config::{Config, DaemonProfile};
use crate::system::daemon::DaemonManager;

pub fn start_background(port: u16, profile: Option<DaemonProfile>) -> Result<()> {
    let config = Config::load_or_create()?;
    let bind_addr = format!("127.0.0.1:{port}");

    // Guard layer 1: PID file says a process is running.
    if is_running()? {
        let pid_file = DaemonManager::get_pid_file_path(&config)?;
        let existing_pid = std::fs::read_to_string(pid_file)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok());
        match existing_pid {
            Some(pid) => println!("  Daemon already running (PID {pid})"),
            None => println!("  Daemon already running"),
        }
        return Ok(());
    }

    // Guard layer 2: something is already answering on this port (e.g. PID
    // file was clobbered by a previous bad start but the daemon is still up).
    if std::net::TcpStream::connect_timeout(
        &bind_addr.parse()?,
        Duration::from_millis(300),
    )
    .is_ok()
    {
        println!("  Daemon already running (port {port} is in use)");
        return Ok(());
    }

    let executable = std::env::current_exe()?;
    let startup_log = startup_log_path();
    let stdout = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&startup_log)?;
    let stderr = stdout.try_clone()?;
    let mut command = std::process::Command::new(&executable);
    command.arg("daemon").args(["--port", &port.to_string()]);
    if let Some(profile) = profile {
        command.args(["--profile", profile.as_str()]);
    }
    command
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .stdin(Stdio::null());

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
    let child_pid = child.id();

    let pid_file = DaemonManager::get_pid_file_path(&config)?;
    if let Some(parent) = pid_file.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Poll for port open. Port was confirmed CLOSED before we spawned so any
    // connect success here comes from the new child, not a pre-existing daemon.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait()? {
            // Child exited before port opened — surface startup log.
            let startup_output = std::fs::read_to_string(&startup_log).unwrap_or_default();
            let startup_output = startup_output.trim();
            if startup_output.is_empty() {
                anyhow::bail!("daemon exited with status {}", status);
            }
            anyhow::bail!("daemon exited with status {}\n{}", status, startup_output);
        }
        if std::net::TcpStream::connect(&bind_addr).is_ok() {
            // Port open AND child alive — child opened the port.
            break;
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            anyhow::bail!(
                "daemon did not open port {port} within 10s — startup log: {}",
                startup_log.display()
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    // Write PID only after child is confirmed healthy (port open, still alive).
    std::fs::write(&pid_file, child_pid.to_string())?;

    println!();
    println!("  {} v{}", APP_DISPLAY_NAME, VERSION);
    println!("  Daemon started (PID {child_pid}, port {port})");
    println!("  Startup log: {}", startup_log.display());
    println!();
    Ok(())
}

pub fn stop() -> Result<()> {
    let config = Config::load_or_create()?;
    let pid_file = DaemonManager::get_pid_file_path(&config)?;
    if !pid_file.exists() {
        let _ = crate::system::runtime_state::clear(&config);
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
    let _ = crate::system::runtime_state::clear(&config);
    let _ = crate::cli::brain::stop_local_server();
    Ok(())
}

pub fn is_running() -> Result<bool> {
    let config = Config::load_or_create()?;
    let pid_file = DaemonManager::get_pid_file_path(&config)?;
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
    Ok(DaemonManager::get_pid_file_path(&config)?)
}

fn startup_log_path() -> PathBuf {
    std::env::temp_dir().join(format!("rove-daemon-start-{}.log", std::process::id()))
}
