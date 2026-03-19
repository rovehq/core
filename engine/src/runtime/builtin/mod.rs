pub mod filesystem;
pub mod terminal;
pub mod vision;

pub use filesystem::FilesystemTool;
pub use terminal::TerminalTool;
pub use vision::VisionTool;

use std::path::PathBuf;

use sdk::errors::EngineError;

use super::registry::ToolRegistry;

pub async fn register_all(
    registry: &mut ToolRegistry,
    workspace: PathBuf,
) -> Result<(), EngineError> {
    registry
        .register_builtin_filesystem(FilesystemTool::new(workspace.clone())?)
        .await;
    registry
        .register_builtin_terminal(TerminalTool::new(workspace.to_string_lossy().to_string()))
        .await;
    registry
        .register_builtin_vision(VisionTool::new(workspace))
        .await;
    Ok(())
}
