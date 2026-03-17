use sdk::errors::EngineError;
use std::collections::HashSet;
use wasmparser::Parser as WasmParser;
use wasmparser::Payload;

use super::WasmRuntime;

impl WasmRuntime {
    pub(super) fn validate_wasm_imports(
        &self,
        wasm_bytes: &[u8],
        allowed_imports: &[String],
        plugin_path: &std::path::Path,
    ) -> Result<(), EngineError> {
        let allowed: HashSet<&str> = allowed_imports.iter().map(|value| value.as_str()).collect();

        let parser = WasmParser::new(0);
        for payload in parser.parse_all(wasm_bytes) {
            let payload = payload.map_err(|error| {
                EngineError::Plugin(format!(
                    "Failed to parse WASM binary {}: {}",
                    plugin_path.display(),
                    error
                ))
            })?;

            if let Payload::ImportSection(reader) = payload {
                for import in reader {
                    let import = import.map_err(|error| {
                        EngineError::Plugin(format!("Failed to read WASM import entry: {}", error))
                    })?;

                    if !allowed.contains(import.module) {
                        return Err(EngineError::Plugin(format!(
                            "Plugin '{}' has forbidden WASM import: '{}'. Allowed: {:?}",
                            plugin_path
                                .file_stem()
                                .and_then(|stem| stem.to_str())
                                .unwrap_or("unknown"),
                            import.module,
                            allowed_imports,
                        )));
                    }
                }
            }
        }

        Ok(())
    }
}
