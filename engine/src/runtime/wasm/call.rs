use sdk::errors::EngineError;

use super::{WasmRuntime, MAX_CRASH_RESTARTS};

impl WasmRuntime {
    pub async fn call_plugin(
        &mut self,
        name: &str,
        function: &str,
        input: &[u8],
    ) -> Result<Vec<u8>, EngineError> {
        tracing::debug!("Calling plugin '{}' function '{}'", name, function);

        self.validate_call_permissions(name, input)?;

        let metadata = self.plugins.get_mut(name).ok_or_else(|| {
            tracing::error!("Plugin '{}' not loaded", name);
            EngineError::PluginNotLoaded(name.to_string())
        })?;

        if metadata.crash_count >= MAX_CRASH_RESTARTS {
            tracing::error!(
                "Plugin '{}' has crashed {} times, refusing to call",
                name,
                metadata.crash_count
            );
            return Err(EngineError::Plugin(format!(
                "Plugin '{}' has crashed too many times ({} crashes)",
                name, metadata.crash_count
            )));
        }

        let result = metadata
            .plugin
            .call::<&[u8], Vec<u8>>(function, input)
            .map_err(|error| {
                tracing::error!("Plugin '{}' function '{}' failed: {}", name, function, error);
                EngineError::Plugin(format!("Plugin call failed: {}", error))
            });

        match result {
            Ok(output) => {
                if metadata.crash_count > 0 {
                    tracing::info!(
                        "Plugin '{}' recovered after {} crashes",
                        name,
                        metadata.crash_count
                    );
                    metadata.crash_count = 0;
                }
                Ok(output)
            }
            Err(error) => {
                self.handle_plugin_crash(name, &error).await?;
                tracing::info!(
                    "Retrying plugin '{}' function '{}' after restart",
                    name,
                    function
                );

                let metadata = self.plugins.get_mut(name).ok_or_else(|| {
                    EngineError::Plugin(format!("Plugin '{}' disappeared after restart", name))
                })?;

                metadata
                    .plugin
                    .call::<&[u8], Vec<u8>>(function, input)
                    .map_err(|retry_error| {
                        tracing::error!(
                            "Plugin '{}' function '{}' failed again after restart: {}",
                            name,
                            function,
                            retry_error
                        );
                        EngineError::Plugin(format!(
                            "Plugin call failed after restart: {}",
                            retry_error
                        ))
                    })
            }
        }
    }

    fn validate_call_permissions(&self, name: &str, input: &[u8]) -> Result<(), EngineError> {
        let plugin_entry = self
            .manifest
            .get_plugin(name)
            .ok_or_else(|| EngineError::PluginNotInManifest(name.to_string()))?;

        if let Ok(input_json) = serde_json::from_slice::<serde_json::Value>(input) {
            if let Some(path) = input_json.get("path").and_then(|value| value.as_str()) {
                if !plugin_entry.is_path_allowed(path) {
                    tracing::warn!(plugin = name, path = path, "Plugin attempted to access denied path");
                    return Err(EngineError::PathDenied(std::path::PathBuf::from(path)));
                }
            }

            if let Some(command) = input_json.get("command").and_then(|value| value.as_str()) {
                if !plugin_entry.is_command_allowed(command) {
                    tracing::warn!(
                        plugin = name,
                        command = command,
                        "Plugin attempted to execute denied command"
                    );
                    return Err(EngineError::CommandNotAllowed(command.to_string()));
                }
            }

            if let Some(size) = input_json.get("size").and_then(|value| value.as_u64()) {
                if let Some(max_size) = plugin_entry.permissions.max_file_size {
                    if size > max_size {
                        tracing::warn!(
                            plugin = name,
                            size = size,
                            max_size = max_size,
                            "Plugin attempted to exceed max file size"
                        );
                        return Err(EngineError::Plugin(format!(
                            "File size {} exceeds maximum allowed size {}",
                            size, max_size
                        )));
                    }
                }
            }
        }

        Ok(())
    }
}
