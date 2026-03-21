use std::path::PathBuf;

use anyhow::Result;
use sdk::core_tool::{CoreContext, CoreTool};
use sdk::errors::EngineError;
use sdk::tool_io::{ToolInput, ToolOutput};
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct VisionTool {
    work_dir: PathBuf,
}

impl VisionTool {
    pub fn new(work_dir: PathBuf) -> Self {
        Self { work_dir }
    }

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
            .arg("-x")
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
            Ok(output) if output.status.success() => Ok(save_path),
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

impl CoreTool for VisionTool {
    fn name(&self) -> &str {
        "vision"
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    fn start(&mut self, ctx: CoreContext) -> Result<(), EngineError> {
        if let Some(workspace) = ctx.config.get("core.workspace").and_then(|v| v.as_str().map(PathBuf::from)) {
            self.work_dir = workspace;
        }
        Ok(())
    }

    fn stop(&mut self) -> Result<(), EngineError> {
        Ok(())
    }

    fn handle(&self, input: ToolInput) -> Result<ToolOutput, EngineError> {
        let output_file = match input.method.as_str() {
            "capture_screen" => input.param_str_opt("output_file").unwrap_or_else(|| "screenshot.png".to_string()),
            other => {
                return Err(EngineError::ToolError(format!(
                    "Unknown vision method '{}'",
                    other
                )))
            }
        };
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|error| EngineError::ToolError(error.to_string()))?;
        let path = runtime
            .block_on(self.capture_screen(&output_file))
            .map_err(|error| EngineError::ToolError(error.to_string()))?;
        Ok(ToolOutput::json(serde_json::json!(path.display().to_string())))
    }
}

#[allow(improper_ctypes_definitions)]
#[no_mangle]
pub extern "C" fn create_tool() -> *mut dyn CoreTool {
    let work_dir = std::env::current_dir().unwrap_or_default();
    Box::into_raw(Box::new(VisionTool::new(work_dir)))
}
