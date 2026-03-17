use anyhow::Result;
use std::path::PathBuf;
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct VisionTool {
    work_dir: PathBuf,
}

impl VisionTool {
    pub fn new(work_dir: PathBuf) -> Self {
        Self { work_dir }
    }

    /// Capture a screenshot and save it to the specified relative or absolute path
    pub async fn capture_screen(&self, output_file: &str) -> Result<PathBuf> {
        let mut save_path = PathBuf::from(output_file);
        if !save_path.is_absolute() {
            save_path = self.work_dir.join(save_path);
        }

        info!("Capturing screenshot to: {}", save_path.display());

        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let save_path_str = save_path.to_string_lossy().to_string();

        #[cfg(target_os = "macos")]
        let result = tokio::process::Command::new("screencapture")
            .arg("-x") // silent
            .arg(&save_path_str)
            .output()
            .await;

        #[cfg(target_os = "linux")]
        let result = tokio::process::Command::new("scrot")
            .arg(&save_path_str)
            .output()
            .await;

        #[cfg(target_os = "windows")]
        let result: std::result::Result<std::process::Output, std::io::Error> =
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Native screenshot not implemented for Windows yet",
            ));

        match result {
            Ok(output) if output.status.success() => {
                info!("Screenshot captured successfully");
                Ok(save_path)
            }
            Ok(output) => {
                let err = String::from_utf8_lossy(&output.stderr);
                warn!("Screenshot command failed: {}", err);
                Err(anyhow::anyhow!("Screenshot failed: {}", err))
            }
            Err(e) => {
                warn!("Failed to execute screenshot utility: {}", e);
                Err(anyhow::anyhow!(
                    "Failed to execute screenshot utility: {}",
                    e
                ))
            }
        }
    }
}
