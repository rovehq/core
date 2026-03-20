use sdk::{
    errors::EngineError,
    types::{ToolInput, ToolOutput},
};
use serde_json::Value;

use super::NativeRuntime;

impl NativeRuntime {
    pub fn unload_tool(&mut self, name: &str) -> Result<(), EngineError> {
        if let Some(lib_path) = self.loaded_tools.remove(name) {
            tracing::info!("Unloading core tool: {}", name);

            if self.loaded_tools.values().any(|loaded| loaded == &lib_path) {
                tracing::debug!(
                    "Keeping native library '{}' loaded because another tool alias still uses it",
                    lib_path
                );
                return Ok(());
            }

            if let Some(mut loaded) = self.loaded_libraries.remove(&lib_path) {
                loaded.tool.stop().map_err(|error| {
                    tracing::error!("Failed to stop tool '{}': {}", name, error);
                    error
                })?;
            }

            tracing::info!("Core tool '{}' unloaded successfully", name);
        } else {
            tracing::debug!("Tool '{}' not loaded, nothing to unload", name);
        }

        Ok(())
    }

    pub fn call_tool(&self, name: &str, input: ToolInput) -> Result<ToolOutput, EngineError> {
        tracing::debug!(
            "Calling core tool '{}' with method '{}'",
            name,
            input.method
        );

        let lib_path = self.loaded_tools.get(name).ok_or_else(|| {
            tracing::error!("Tool '{}' not loaded", name);
            EngineError::ToolNotLoaded(name.to_string())
        })?;
        let tool = &self
            .loaded_libraries
            .get(lib_path)
            .ok_or_else(|| EngineError::ToolNotLoaded(name.to_string()))?
            .tool;

        tool.handle(input).map_err(|error| {
            tracing::error!("Tool '{}' returned error: {}", name, error);
            error
        })
    }

    pub fn is_tool_loaded(&self, name: &str) -> bool {
        self.loaded_tools.contains_key(name)
    }

    pub fn loaded_tools(&self) -> Vec<String> {
        self.loaded_tools.keys().cloned().collect()
    }

    pub fn call_registered_tool(
        &mut self,
        lib_path: &str,
        tool_name: &str,
        args: Value,
    ) -> Result<Value, EngineError> {
        if !self.loaded_libraries.contains_key(lib_path) {
            self.load_registered_library(lib_path, tool_name)?;
        } else {
            self.loaded_tools
                .entry(tool_name.to_string())
                .or_insert_with(|| lib_path.to_string());
        }

        let input = tool_input_from_args(tool_name, args);
        let output = self.call_tool(tool_name, input)?;
        tool_output_to_value(tool_name, output)
    }

    pub fn unload_all(&mut self) {
        tracing::info!("Unloading all core tools");

        self.loaded_tools.clear();

        for (lib_path, mut loaded) in std::mem::take(&mut self.loaded_libraries) {
            if let Err(error) = loaded.tool.stop() {
                tracing::error!("Error unloading native library '{}': {}", lib_path, error);
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

fn tool_input_from_args(tool_name: &str, args: Value) -> ToolInput {
    let mut input = ToolInput::new(tool_name);

    match args {
        Value::Object(map) => {
            for (key, value) in map {
                input = input.with_param(key, value);
            }
        }
        other => {
            input = input.with_param("value", other);
        }
    }

    input
}

fn tool_output_to_value(tool_name: &str, output: ToolOutput) -> Result<Value, EngineError> {
    if output.success {
        Ok(output.data)
    } else {
        Err(EngineError::ToolError(output.error.unwrap_or_else(|| {
            format!("native tool '{}' returned an unspecified error", tool_name)
        })))
    }
}
