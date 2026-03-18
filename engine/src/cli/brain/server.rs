use anyhow::{Context, Result};
use brain::reasoning::LocalBrain;
use serde::{Deserialize, Serialize};
use std::net::{SocketAddr, TcpStream};
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ServerMetadata {
    pub pid: u32,
    pub model_path: String,
    pub port: u16,
    pub adapter_path: Option<String>,
}

pub fn start(model: Option<&str>, port: u16) -> Result<()> {
    println!("Starting llama-server...");
    println!();

    let llama_server = which::which("llama-server").context("llama-server not found in PATH")?;
    let brain_dir = LocalBrain::default_brain_dir().context("Failed to get brain directory")?;
    std::fs::create_dir_all(&brain_dir).context("Failed to create brain directory")?;
    if let Some(metadata) = read_metadata()? {
        if is_pid_running(metadata.pid) {
            println!("llama-server is already running");
            println!("PID:   {}", metadata.pid);
            println!("Model: {}", metadata.model_path);
            println!("URL:   http://localhost:{}", metadata.port);
            return Ok(());
        }
        remove_metadata_file()?;
    }
    let model_path = resolve_model_path(&brain_dir, model)?;

    let mut command = Command::new(llama_server);
    command
        .arg("--model")
        .arg(&model_path)
        .arg("--port")
        .arg(port.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null());

    let adapter_path = LocalBrain::adapter_path().filter(|path| path.exists());
    if let Some(adapter_path) = &adapter_path {
        command.arg("--lora").arg(adapter_path);
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    let mut child = command.spawn().context("Failed to start llama-server")?;
    std::thread::sleep(Duration::from_millis(250));
    if let Some(status) = child.try_wait()? {
        anyhow::bail!("llama-server exited immediately with status {}", status);
    }

    let metadata = ServerMetadata {
        pid: child.id(),
        model_path: model_path.display().to_string(),
        port,
        adapter_path: adapter_path.map(|path| path.display().to_string()),
    };
    if let Err(error) = wait_for_port(metadata.port) {
        let _ = stop_pid(metadata.pid);
        return Err(error);
    }
    write_metadata(&metadata)?;

    println!("llama-server started");
    println!("PID:   {}", metadata.pid);
    println!("Model: {}", metadata.model_path);
    println!("URL:   http://localhost:{}", metadata.port);
    println!(
        "LoRA:  {}",
        metadata.adapter_path.as_deref().unwrap_or("not loaded")
    );

    Ok(())
}

pub fn stop() -> Result<()> {
    stop_background()
}

pub(crate) fn stop_background() -> Result<()> {
    let metadata = read_metadata()?;

    if let Some(metadata) = metadata {
        if !is_pid_running(metadata.pid) {
            remove_metadata_file()?;
            println!("No running llama-server found");
            return Ok(());
        }
        stop_pid(metadata.pid)?;
        remove_metadata_file()?;
        println!("llama-server stopped");
        return Ok(());
    }

    #[cfg(unix)]
    {
        let output = Command::new("pkill")
            .arg("llama-server")
            .output()
            .context("Failed to run pkill")?;
        if output.status.success() {
            println!("llama-server stopped");
        } else {
            println!("No running llama-server found");
        }
    }

    #[cfg(not(unix))]
    {
        println!("Stop is not implemented on this platform.");
        println!("Stop llama-server manually.");
    }

    Ok(())
}

pub(crate) fn read_metadata() -> Result<Option<ServerMetadata>> {
    let path = metadata_path()?;
    if !path.exists() {
        return Ok(None);
    }

    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let metadata: ServerMetadata =
        serde_json::from_str(&raw).context("Failed to parse llama-server metadata")?;
    Ok(Some(metadata))
}

fn write_metadata(metadata: &ServerMetadata) -> Result<()> {
    let path = metadata_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_vec_pretty(metadata)?)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

fn remove_metadata_file() -> Result<()> {
    let path = metadata_path()?;
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

fn metadata_path() -> Result<std::path::PathBuf> {
    let brain_dir = LocalBrain::default_brain_dir().context("Failed to get brain directory")?;
    Ok(brain_dir.join("llama-server.json"))
}

fn resolve_model_path(
    brain_dir: &std::path::Path,
    model: Option<&str>,
) -> Result<std::path::PathBuf> {
    if let Some(model) = model {
        let candidate = std::path::PathBuf::from(model);
        if candidate.exists() {
            return Ok(candidate);
        }

        let candidate = brain_dir.join(format!("{}.gguf", model));
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    let entries = std::fs::read_dir(brain_dir).context("Failed to read brain directory")?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("gguf") {
            continue;
        }
        if path.file_name().and_then(|name| name.to_str()) == Some("adapter.gguf") {
            continue;
        }
        return Ok(path);
    }

    anyhow::bail!("No models found. Install a model with `rove brain install <model>`.")
}

fn stop_pid(pid: u32) -> Result<()> {
    #[cfg(unix)]
    unsafe {
        if libc::kill(pid as i32, libc::SIGTERM) != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                return Ok(());
            }
            return Err(error).context("Failed to stop llama-server");
        }

        for _ in 0..50 {
            if !is_pid_running(pid) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        if libc::kill(pid as i32, libc::SIGKILL) != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                return Ok(());
            }
            return Err(error).context("Failed to force-stop llama-server");
        }
    }

    #[cfg(windows)]
    {
        let status = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .status()
            .context("Failed to run taskkill")?;
        if !status.success() {
            anyhow::bail!("taskkill failed for PID {}", pid);
        }
    }

    Ok(())
}

fn wait_for_port(port: u16) -> Result<()> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    for _ in 0..50 {
        if TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_ok() {
            std::thread::sleep(Duration::from_millis(300));
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    anyhow::bail!("llama-server did not open port {}", port);
}

#[cfg(unix)]
fn is_pid_running(pid: u32) -> bool {
    unsafe {
        if libc::kill(pid as i32, 0) == 0 {
            true
        } else {
            std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
        }
    }
}

#[cfg(not(unix))]
fn is_pid_running(_pid: u32) -> bool {
    false
}
