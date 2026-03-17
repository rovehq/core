use sdk::{
    errors::EngineError,
    types::{ToolInput, ToolOutput},
};

use super::NativeRuntime;

impl NativeRuntime {
    pub fn unload_tool(&mut self, name: &str) -> Result<(), EngineError> {
        if let Some(mut tool) = self.tools.remove(name) {
            tracing::info!("Unloading core tool: {}", name);

            tool.stop().map_err(|error| {
                tracing::error!("Failed to stop tool '{}': {}", name, error);
                error
            })?;

            self.libraries.remove(name);
            tracing::info!("Core tool '{}' unloaded successfully", name);
        } else {
            tracing::debug!("Tool '{}' not loaded, nothing to unload", name);
        }

        Ok(())
    }

    pub fn call_tool(&self, name: &str, input: ToolInput) -> Result<ToolOutput, EngineError> {
        tracing::debug!("Calling core tool '{}' with method '{}'", name, input.method);

        let tool = self.tools.get(name).ok_or_else(|| {
            tracing::error!("Tool '{}' not loaded", name);
            EngineError::ToolNotLoaded(name.to_string())
        })?;

        tool.handle(input).map_err(|error| {
            tracing::error!("Tool '{}' returned error: {}", name, error);
            error
        })
    }

    pub fn is_tool_loaded(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    pub fn loaded_tools(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    pub fn unload_all(&mut self) {
        tracing::info!("Unloading all core tools");

        let tool_names: Vec<String> = self.tools.keys().cloned().collect();
        for name in tool_names {
            if let Err(error) = self.unload_tool(&name) {
                tracing::error!("Error unloading tool '{}': {}", name, error);
            }
        }

        tracing::info!("All core tools unloaded");
    }
}

impl Drop for NativeRuntime {
    fn drop(&mut self) {
        self.unload_all();
    }
}
