use std::path::Path;

use sdk::{
    core_tool::{CoreContext, CoreTool},
    errors::EngineError,
};

use super::NativeRuntime;

impl NativeRuntime {
    pub fn load_tool(&mut self, name: &str, ctx: CoreContext) -> Result<(), EngineError> {
        tracing::info!("Loading core tool: {}", name);

        let tool_path = self.verified_tool_path(name)?;
        tracing::info!("All four gates passed for '{}', loading library...", name);

        let library = unsafe { self.load_library(&tool_path)? };
        let mut tool = unsafe { Self::create_tool_instance(&library, name, &tool_path)? };

        tool.start(ctx).map_err(|error| {
            tracing::error!("Failed to start tool '{}': {}", name, error);
            error
        })?;

        self.tools.insert(name.to_string(), tool);
        self.libraries.insert(name.to_string(), library);

        tracing::info!("Core tool '{}' loaded successfully", name);
        Ok(())
    }

    unsafe fn load_library(&self, tool_path: &Path) -> Result<libloading::Library, EngineError> {
        libloading::Library::new(tool_path).map_err(|error| {
            tracing::error!("Failed to load library {}: {}", tool_path.display(), error);
            EngineError::LibraryLoadFailed(error.to_string())
        })
    }

    unsafe fn create_tool_instance(
        library: &libloading::Library,
        name: &str,
        tool_path: &Path,
    ) -> Result<Box<dyn CoreTool>, EngineError> {
        let create_tool: libloading::Symbol<unsafe extern "C" fn() -> *mut dyn CoreTool> =
            library.get(b"create_tool").map_err(|error| {
                tracing::error!(
                    "Symbol 'create_tool' not found in {}: {}",
                    tool_path.display(),
                    error
                );
                EngineError::SymbolNotFound(error.to_string())
            })?;

        let ptr = create_tool();
        if ptr.is_null() {
            tracing::error!("create_tool returned null pointer for '{}'", name);
            return Err(EngineError::LibraryLoadFailed(
                "create_tool returned null".to_string(),
            ));
        }

        Ok(Box::from_raw(ptr))
    }
}
