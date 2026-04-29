pub mod browser;

pub use browser::BrowserTool;
pub use filesystem::FilesystemTool;
pub use terminal::TerminalTool;

use std::path::PathBuf;

use sdk::errors::EngineError;

use super::registry::ToolRegistry;

#[derive(Debug, Clone, Copy)]
pub struct BuiltinSelection {
    pub filesystem: bool,
    pub terminal: bool,
}

impl BuiltinSelection {
    pub fn all() -> Self {
        Self {
            filesystem: true,
            terminal: true,
        }
    }

    pub fn kernel_defaults() -> Self {
        Self {
            filesystem: true,
            terminal: true,
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
    registry.register_builtin_web_fetch().await;
    registry.register_builtin_web_search().await;
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
    Ok(())
}
