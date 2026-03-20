use std::path::Path;

use sdk::{
    core_tool::{CoreContext, CoreTool},
    errors::EngineError,
};

use super::{LoadedLibrary, NativeRuntime};

impl NativeRuntime {
    pub fn load_tool(&mut self, name: &str, ctx: CoreContext) -> Result<(), EngineError> {
        tracing::info!("Loading core tool: {}", name);

        let tool_path = self.verified_tool_path(name)?;
        self.ctx = ctx;
        self.load_library_path(tool_path, name)
    }

    pub(super) fn load_registered_library(
        &mut self,
        lib_path: &str,
        tool_name: &str,
    ) -> Result<(), EngineError> {
        let tool_path = self.verify_registered_library_path(lib_path)?;
        self.load_library_path(tool_path, tool_name)
    }

    fn load_library_path(
        &mut self,
        tool_path: std::path::PathBuf,
        tool_name: &str,
    ) -> Result<(), EngineError> {
        let library_key = tool_path.display().to_string();
        if self.loaded_libraries.contains_key(&library_key) {
            self.loaded_tools
                .insert(tool_name.to_string(), library_key.to_string());
            return Ok(());
        }

        tracing::info!(
            "Native verification passed for '{}', loading {}",
            tool_name,
            tool_path.display()
        );

        let library = unsafe { self.load_library(&tool_path)? };
        let mut tool = unsafe { Self::create_tool_instance(&library, tool_name, &tool_path)? };

        tool.start(self.ctx.clone()).map_err(|error| {
            tracing::error!("Failed to start tool '{}': {}", tool_name, error);
            error
        })?;

        self.loaded_tools
            .insert(tool_name.to_string(), library_key.clone());
        self.loaded_libraries
            .insert(library_key, LoadedLibrary { tool, library });

        tracing::info!("Core tool '{}' loaded successfully", tool_name);
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
