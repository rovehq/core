use crate::message_bus::Event;
use sdk::errors::EngineError;

use super::{WasmRuntime, MAX_CRASH_RESTARTS};

impl WasmRuntime {
    pub(super) async fn handle_plugin_crash(
        &mut self,
        name: &str,
        error: &EngineError,
    ) -> Result<(), EngineError> {
        if let Some(metadata) = self.plugins.get_mut(name) {
            metadata.crash_count += 1;
            let crash_count = metadata.crash_count;

            tracing::error!(
                "Plugin '{}' crashed (crash #{}/{}): {}",
                name,
                crash_count,
                MAX_CRASH_RESTARTS,
                error
            );

            if let Some(bus) = &self.message_bus {
                bus.publish(Event::PluginCrashed {
                    plugin_id: name.to_string(),
                    error: format!("Crash #{}: {}", crash_count, error),
                })
                .await;
            }

            if crash_count >= MAX_CRASH_RESTARTS {
                tracing::error!(
                    "Plugin '{}' has reached maximum crash limit ({}), will not restart",
                    name,
                    MAX_CRASH_RESTARTS
                );
                return Err(EngineError::Plugin(format!(
                    "Plugin '{}' has crashed {} times and will not be restarted",
                    name, MAX_CRASH_RESTARTS
                )));
            }

            tracing::warn!(
                "Attempting to restart plugin '{}' (crash #{}/{})",
                name,
                crash_count,
                MAX_CRASH_RESTARTS
            );

            let crash_count_backup = crash_count;
            self.plugins.remove(name);
            self.load_plugin(name).await?;

            if let Some(metadata) = self.plugins.get_mut(name) {
                metadata.crash_count = crash_count_backup;
            }

            tracing::info!("Plugin '{}' restarted successfully after crash", name);
            Ok(())
        } else {
            Err(EngineError::PluginNotLoaded(name.to_string()))
        }
    }

    pub fn unload_plugin(&mut self, name: &str) {
        if self.plugins.remove(name).is_some() {
            tracing::info!("Plugin '{}' unloaded", name);
        } else {
            tracing::debug!("Plugin '{}' not loaded, nothing to unload", name);
        }
    }

    pub async fn restart_plugin(&mut self, name: &str) -> Result<(), EngineError> {
        tracing::warn!("Manually restarting plugin: {}", name);

        self.plugins.remove(name);
        self.load_plugin(name).await?;

        if let Some(metadata) = self.plugins.get_mut(name) {
            metadata.crash_count = 0;
        }

        tracing::info!("Plugin '{}' restarted successfully", name);
        Ok(())
    }

    pub fn get_crash_count(&self, name: &str) -> Option<u32> {
        self.plugins.get(name).map(|metadata| metadata.crash_count)
    }

    pub fn is_plugin_loaded(&self, name: &str) -> bool {
        self.plugins.contains_key(name)
    }

    pub fn loaded_plugins(&self) -> Vec<String> {
        self.plugins.keys().cloned().collect()
    }

    pub fn unload_all(&mut self) {
        tracing::info!("Unloading all plugins");
        let plugin_names: Vec<String> = self.plugins.keys().cloned().collect();
        for name in plugin_names {
            self.unload_plugin(&name);
        }
        tracing::info!("All plugins unloaded");
    }
}

impl Drop for WasmRuntime {
    fn drop(&mut self) {
        self.unload_all();
    }
}
