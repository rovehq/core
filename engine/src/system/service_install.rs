use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::config::{metadata::DEFAULT_PORT, Config, DaemonProfile};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceInstallMode {
    Login,
    Boot,
}

impl ServiceInstallMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::Boot => "boot",
        }
    }

    pub fn default_profile(&self) -> DaemonProfile {
        match self {
            Self::Login => DaemonProfile::Desktop,
            Self::Boot => DaemonProfile::Edge,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceInstallState {
    pub mode: String,
    pub installed: bool,
    pub supported: bool,
    pub path: String,
    pub label: String,
    pub default_profile: String,
    pub auto_restart: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceInstallStatus {
    pub current_binary: Option<String>,
    pub default_port: u16,
    pub login: ServiceInstallState,
    pub boot: ServiceInstallState,
}

pub struct ServiceInstaller {
    config: Config,
}

impl ServiceInstaller {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub fn status(&self) -> Result<ServiceInstallStatus> {
        Ok(ServiceInstallStatus {
            current_binary: current_binary_path(),
            default_port: DEFAULT_PORT,
            login: self.mode_status(ServiceInstallMode::Login),
            boot: self.mode_status(ServiceInstallMode::Boot),
        })
    }

    pub fn install(
        &self,
        mode: ServiceInstallMode,
        profile: Option<DaemonProfile>,
        port: u16,
    ) -> Result<ServiceInstallState> {
        let profile = profile.unwrap_or_else(|| mode.default_profile());
        let binary = current_binary_path()
            .ok_or_else(|| anyhow::anyhow!("Could not determine the current `rove` binary path"))?;
        let descriptor = service_descriptor(mode)?;
        if let Some(parent) = descriptor.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        let logs_dir = logs_dir(&self.config, mode);
        fs::create_dir_all(&logs_dir)
            .with_context(|| format!("Failed to create {}", logs_dir.display()))?;

        let content =
            render_service_file(mode, &descriptor.label, &binary, port, profile, &logs_dir);
        fs::write(&descriptor.path, content)
            .with_context(|| format!("Failed to write {}", descriptor.path.display()))?;
        lock_down_service_file(&descriptor.path)?;
        activate_service(mode, &descriptor)?;

        Ok(self.mode_status(mode))
    }

    pub fn uninstall(&self, mode: ServiceInstallMode) -> Result<()> {
        let descriptor = service_descriptor(mode)?;
        deactivate_service(mode, &descriptor)?;
        if descriptor.path.exists() {
            fs::remove_file(&descriptor.path)
                .with_context(|| format!("Failed to remove {}", descriptor.path.display()))?;
        }
        Ok(())
    }

    fn mode_status(&self, mode: ServiceInstallMode) -> ServiceInstallState {
        match service_descriptor(mode) {
            Ok(descriptor) => ServiceInstallState {
                mode: mode.as_str().to_string(),
                installed: descriptor.path.exists(),
                supported: true,
                path: descriptor.path.display().to_string(),
                label: descriptor.label,
                default_profile: mode.default_profile().as_str().to_string(),
                auto_restart: true,
            },
            Err(_) => ServiceInstallState {
                mode: mode.as_str().to_string(),
                installed: false,
                supported: false,
                path: String::new(),
                label: String::new(),
                default_profile: mode.default_profile().as_str().to_string(),
                auto_restart: false,
            },
        }
    }
}

#[derive(Debug, Clone)]
struct ServiceDescriptor {
    label: String,
    path: PathBuf,
}

fn current_binary_path() -> Option<String> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.into_os_string().into_string().ok())
}

fn logs_dir(config: &Config, mode: ServiceInstallMode) -> PathBuf {
    let root = config
        .core
        .data_dir
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| config.core.data_dir.clone());
    root.join("logs").join(mode.as_str())
}

#[cfg(target_os = "macos")]
fn service_descriptor(mode: ServiceInstallMode) -> Result<ServiceDescriptor> {
    let label = format!("co.roveai.daemon.{}", mode.as_str());
    let path = match mode {
        ServiceInstallMode::Login => dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine the home directory"))?
            .join("Library")
            .join("LaunchAgents")
            .join(format!("{}.plist", label)),
        ServiceInstallMode::Boot => {
            PathBuf::from("/Library/LaunchDaemons").join(format!("{}.plist", label))
        }
    };
    Ok(ServiceDescriptor { label, path })
}

#[cfg(target_os = "linux")]
fn service_descriptor(mode: ServiceInstallMode) -> Result<ServiceDescriptor> {
    let label = format!("co.roveai.daemon-{}", mode.as_str());
    let path = match mode {
        ServiceInstallMode::Login => dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine the home directory"))?
            .join(".config")
            .join("systemd")
            .join("user")
            .join(format!("{}.service", label)),
        ServiceInstallMode::Boot => {
            PathBuf::from("/etc/systemd/system").join(format!("{}.service", label))
        }
    };
    Ok(ServiceDescriptor { label, path })
}

#[cfg(target_os = "windows")]
fn service_descriptor(mode: ServiceInstallMode) -> Result<ServiceDescriptor> {
    let label = format!("co.roveai.daemon-{}", mode.as_str());
    let path = match mode {
        ServiceInstallMode::Login => dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine the Roaming AppData directory"))?
            .join("Rove")
            .join(format!("{}.cmd", label)),
        ServiceInstallMode::Boot => {
            let program_data = std::env::var("ProgramData")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from(r"C:\ProgramData"));
            program_data.join("Rove").join(format!("{}.cmd", label))
        }
    };
    Ok(ServiceDescriptor { label, path })
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn service_descriptor(_mode: ServiceInstallMode) -> Result<ServiceDescriptor> {
    bail!("Service installation is not implemented on this platform yet")
}

#[allow(unused_variables)]
fn render_service_file(
    _mode: ServiceInstallMode,
    label: &str,
    binary: &str,
    port: u16,
    profile: DaemonProfile,
    logs_dir: &std::path::Path,
) -> String {
    #[cfg(target_os = "macos")]
    {
        let stdout = logs_dir.join("daemon.stdout.log").display().to_string();
        let stderr = logs_dir.join("daemon.stderr.log").display().to_string();
        return format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{binary}</string>
    <string>daemon</string>
    <string>--port</string>
    <string>{port}</string>
    <string>--profile</string>
    <string>{profile}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{stdout}</string>
  <key>StandardErrorPath</key>
  <string>{stderr}</string>
</dict>
</plist>
"#,
            label = label,
            binary = binary,
            port = port,
            profile = profile.as_str(),
            stdout = stdout,
            stderr = stderr,
        );
    }

    #[cfg(target_os = "linux")]
    {
        let stdout = logs_dir.join("daemon.stdout.log").display().to_string();
        let stderr = logs_dir.join("daemon.stderr.log").display().to_string();
        return format!(
            r#"[Unit]
Description=Rove daemon ({mode})
After=network.target

[Service]
Type=simple
ExecStart={binary} daemon --port {port} --profile {profile}
Restart=always
RestartSec=2
StandardOutput=append:{stdout}
StandardError=append:{stderr}

[Install]
WantedBy=default.target
"#,
            mode = _mode.as_str(),
            binary = binary,
            port = port,
            profile = profile.as_str(),
            stdout = stdout,
            stderr = stderr,
        );
    }

    #[cfg(target_os = "windows")]
    {
        let stdout = logs_dir.join("daemon.stdout.log").display().to_string();
        let stderr = logs_dir.join("daemon.stderr.log").display().to_string();
        // Wrapper cmd is the install marker; actual activation uses sc.exe / schtasks.
        return format!(
            "@echo off\r\nrem Rove daemon wrapper ({mode}) — label {label}\r\n\"{binary}\" daemon --port {port} --profile {profile} 1>>\"{stdout}\" 2>>\"{stderr}\"\r\n",
            mode = _mode.as_str(),
            label = label,
            binary = binary,
            port = port,
            profile = profile.as_str(),
            stdout = stdout,
            stderr = stderr,
        );
    }

    #[allow(unreachable_code)]
    String::new()
}

#[cfg(target_os = "macos")]
fn activate_service(mode: ServiceInstallMode, descriptor: &ServiceDescriptor) -> Result<()> {
    let domain = match mode {
        ServiceInstallMode::Login => format!("gui/{}", unsafe { libc::getuid() }),
        ServiceInstallMode::Boot => "system".to_string(),
    };
    run_command(
        "/bin/launchctl",
        &["bootout", &domain, &descriptor.path.display().to_string()],
        true,
    )?;
    run_command(
        "/bin/launchctl",
        &["bootstrap", &domain, &descriptor.path.display().to_string()],
        false,
    )?;
    run_command(
        "/bin/launchctl",
        &["enable", &format!("{}/{}", domain, descriptor.label)],
        true,
    )?;
    run_command(
        "/bin/launchctl",
        &[
            "kickstart",
            "-k",
            &format!("{}/{}", domain, descriptor.label),
        ],
        true,
    )?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn deactivate_service(mode: ServiceInstallMode, descriptor: &ServiceDescriptor) -> Result<()> {
    let domain = match mode {
        ServiceInstallMode::Login => format!("gui/{}", unsafe { libc::getuid() }),
        ServiceInstallMode::Boot => "system".to_string(),
    };
    run_command(
        "/bin/launchctl",
        &["bootout", &domain, &descriptor.path.display().to_string()],
        true,
    )?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn activate_service(mode: ServiceInstallMode, descriptor: &ServiceDescriptor) -> Result<()> {
    let user = matches!(mode, ServiceInstallMode::Login);
    run_systemctl(user, &["daemon-reload"], false)?;
    run_systemctl(user, &["enable", "--now", &descriptor.label], false)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn deactivate_service(mode: ServiceInstallMode, descriptor: &ServiceDescriptor) -> Result<()> {
    let user = matches!(mode, ServiceInstallMode::Login);
    run_systemctl(user, &["disable", "--now", &descriptor.label], true)?;
    run_systemctl(user, &["daemon-reload"], true)?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn activate_service(mode: ServiceInstallMode, descriptor: &ServiceDescriptor) -> Result<()> {
    let wrapper = descriptor.path.display().to_string();
    match mode {
        ServiceInstallMode::Boot => {
            // SCM service; auto-start at boot, restart on failure.
            run_command("sc.exe", &["stop", &descriptor.label], true)?;
            run_command("sc.exe", &["delete", &descriptor.label], true)?;
            let bin_path = format!("cmd.exe /c \"{}\"", wrapper);
            run_command(
                "sc.exe",
                &[
                    "create",
                    &descriptor.label,
                    "binPath=",
                    &bin_path,
                    "start=",
                    "auto",
                    "DisplayName=",
                    "Rove Daemon",
                ],
                false,
            )?;
            run_command(
                "sc.exe",
                &[
                    "failure",
                    &descriptor.label,
                    "reset=",
                    "60",
                    "actions=",
                    "restart/2000/restart/2000/restart/2000",
                ],
                true,
            )?;
            run_command("sc.exe", &["start", &descriptor.label], false)?;
        }
        ServiceInstallMode::Login => {
            // Per-user Task Scheduler job at logon.
            run_command(
                "schtasks.exe",
                &["/Delete", "/TN", &descriptor.label, "/F"],
                true,
            )?;
            run_command(
                "schtasks.exe",
                &[
                    "/Create",
                    "/TN",
                    &descriptor.label,
                    "/TR",
                    &wrapper,
                    "/SC",
                    "ONLOGON",
                    "/RL",
                    "LIMITED",
                    "/F",
                ],
                false,
            )?;
            run_command("schtasks.exe", &["/Run", "/TN", &descriptor.label], true)?;
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn deactivate_service(mode: ServiceInstallMode, descriptor: &ServiceDescriptor) -> Result<()> {
    match mode {
        ServiceInstallMode::Boot => {
            run_command("sc.exe", &["stop", &descriptor.label], true)?;
            run_command("sc.exe", &["delete", &descriptor.label], true)?;
        }
        ServiceInstallMode::Login => {
            run_command("schtasks.exe", &["/End", "/TN", &descriptor.label], true)?;
            run_command(
                "schtasks.exe",
                &["/Delete", "/TN", &descriptor.label, "/F"],
                true,
            )?;
        }
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn activate_service(_mode: ServiceInstallMode, _descriptor: &ServiceDescriptor) -> Result<()> {
    bail!("Service installation is not implemented on this platform yet")
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn deactivate_service(_mode: ServiceInstallMode, _descriptor: &ServiceDescriptor) -> Result<()> {
    bail!("Service installation is not implemented on this platform yet")
}

fn run_command(binary: &str, args: &[&str], ignore_failure: bool) -> Result<()> {
    let status = Command::new(binary)
        .args(args)
        .status()
        .with_context(|| format!("Failed to run `{}`", binary))?;
    if !status.success() && !ignore_failure {
        bail!(
            "`{}` exited with status {}",
            std::iter::once(binary)
                .chain(args.iter().copied())
                .collect::<Vec<_>>()
                .join(" "),
            status
        );
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn run_systemctl(user: bool, args: &[&str], ignore_failure: bool) -> Result<()> {
    let mut full = Vec::with_capacity(args.len() + usize::from(user));
    if user {
        full.push("--user");
    }
    full.extend_from_slice(args);
    run_command("/bin/systemctl", &full, ignore_failure)
}

#[allow(unused_variables)]
fn lock_down_service_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o644))
            .with_context(|| format!("Failed to chmod {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_defaults_to_desktop() {
        assert_eq!(
            ServiceInstallMode::Login.default_profile(),
            DaemonProfile::Desktop
        );
        assert_eq!(
            ServiceInstallMode::Boot.default_profile(),
            DaemonProfile::Edge
        );
    }

    #[test]
    fn rendered_service_file_includes_profile_and_port() {
        let port = DEFAULT_PORT;
        let output = render_service_file(
            ServiceInstallMode::Login,
            "co.roveai.daemon.login",
            "/usr/local/bin/rove",
            port,
            DaemonProfile::Desktop,
            &PathBuf::from("/tmp/rove-logs"),
        );
        assert!(output.contains("/usr/local/bin/rove"));
        assert!(output.contains(&port.to_string()));
        assert!(output.contains("desktop"));
    }
}
