pub use filesystem::FilesystemTool;
pub use screenshot::VisionTool;
pub use terminal::TerminalTool;

use std::path::PathBuf;

use crate::config::Config;
use sdk::errors::EngineError;

use super::registry::ToolRegistry;

#[derive(Debug, Clone, Copy)]
pub struct BuiltinSelection {
    pub filesystem: bool,
    pub terminal: bool,
    pub vision: bool,
}

impl BuiltinSelection {
    pub fn all() -> Self {
        Self {
            filesystem: true,
            terminal: true,
            vision: true,
        }
    }

    pub fn from_config(config: &Config) -> Self {
        Self {
            filesystem: config.plugins.fs_editor,
            terminal: config.plugins.terminal,
            vision: config.plugins.screenshot,
        }
    }
}

pub async fn register_all(
    registry: &mut ToolRegistry,
    workspace: PathBuf,
) -> Result<(), EngineError> {
    register_selected(registry, workspace, BuiltinSelection::all()).await
}

pub async fn register_selected(
    registry: &mut ToolRegistry,
    workspace: PathBuf,
    selection: BuiltinSelection,
) -> Result<(), EngineError> {
    if selection.filesystem {
        registry
            .register_builtin_filesystem(FilesystemTool::new(workspace.clone())?)
            .await;
    }
    if selection.terminal {
        registry
            .register_builtin_terminal(TerminalTool::new(workspace.to_string_lossy().to_string()))
            .await;
    }
    if selection.vision {
        registry
            .register_builtin_vision(VisionTool::new(workspace))
            .await;
    }
    Ok(())
}
