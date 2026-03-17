use crate::fs_guard::FileSystemGuard;
use extism::{Function, UserData};
use std::sync::Arc;

use super::WasmRuntime;

impl WasmRuntime {
    pub(super) fn create_host_functions(&self) -> Vec<Function> {
        let f_read = Function::new(
            "host_read_file",
            [extism::ValType::I64],
            [extism::ValType::I64],
            UserData::new(self.fs_guard.clone()),
            |plugin: &mut extism::CurrentPlugin,
             inputs: &[extism::Val],
             outputs: &mut [extism::Val],
             user_data: UserData<Arc<FileSystemGuard>>|
             -> Result<(), extism::Error> {
                let path_offset = inputs[0].unwrap_i64() as u64;
                let path_handle = plugin
                    .memory_handle(path_offset)
                    .ok_or_else(|| extism::Error::msg("Invalid path offset"))?;
                let path_str = plugin.memory_str(path_handle)?.to_string();
                let binding = user_data.get()?;
                let guard = binding.lock().map_err(|error| {
                    extism::Error::msg(format!("FileSystemGuard lock poisoned: {}", error))
                })?;
                let content = match guard.validate_path(std::path::Path::new(&path_str)) {
                    Ok(path) => {
                        std::fs::read_to_string(path).unwrap_or_else(|error| format!("Error: {}", error))
                    }
                    Err(error) => format!("Error: {}", error),
                };
                let out_handle = plugin.memory_new(&content)?;
                outputs[0] = extism::Val::I64(out_handle.offset() as i64);
                Ok(())
            },
        );

        let f_write = Function::new(
            "host_write_file",
            [extism::ValType::I64, extism::ValType::I64],
            [extism::ValType::I64],
            UserData::new(self.fs_guard.clone()),
            |plugin: &mut extism::CurrentPlugin,
             inputs: &[extism::Val],
             outputs: &mut [extism::Val],
             user_data: UserData<Arc<FileSystemGuard>>|
             -> Result<(), extism::Error> {
                let path_offset = inputs[0].unwrap_i64() as u64;
                let path_handle = plugin
                    .memory_handle(path_offset)
                    .ok_or_else(|| extism::Error::msg("Invalid path offset"))?;
                let content_offset = inputs[1].unwrap_i64() as u64;
                let content_handle = plugin
                    .memory_handle(content_offset)
                    .ok_or_else(|| extism::Error::msg("Invalid content offset"))?;

                let path_str = plugin.memory_str(path_handle)?.to_string();
                let content_str = plugin.memory_str(content_handle)?.to_string();
                let binding = user_data.get()?;
                let guard = binding.lock().map_err(|error| {
                    extism::Error::msg(format!("FileSystemGuard lock poisoned: {}", error))
                })?;
                let status = match guard.validate_path(std::path::Path::new(&path_str)) {
                    Ok(path) => {
                        if let Err(error) = std::fs::write(path, content_str) {
                            format!("Error: {}", error)
                        } else {
                            "ok".to_string()
                        }
                    }
                    Err(error) => format!("Error: {}", error),
                };
                let out_handle = plugin.memory_new(&status)?;
                outputs[0] = extism::Val::I64(out_handle.offset() as i64);
                Ok(())
            },
        );

        let f_delete = Function::new(
            "host_delete_file",
            [extism::ValType::I64],
            [extism::ValType::I64],
            UserData::new(self.fs_guard.clone()),
            |plugin: &mut extism::CurrentPlugin,
             inputs: &[extism::Val],
             outputs: &mut [extism::Val],
             user_data: UserData<Arc<FileSystemGuard>>|
             -> Result<(), extism::Error> {
                let path_offset = inputs[0].unwrap_i64() as u64;
                let path_handle = plugin
                    .memory_handle(path_offset)
                    .ok_or_else(|| extism::Error::msg("Invalid path offset"))?;
                let path_str = plugin.memory_str(path_handle)?.to_string();
                let binding = user_data.get()?;
                let guard = binding.lock().map_err(|error| {
                    extism::Error::msg(format!("FileSystemGuard lock poisoned: {}", error))
                })?;
                let status = match guard.validate_path(std::path::Path::new(&path_str)) {
                    Ok(path) => {
                        if let Err(error) = std::fs::remove_file(path) {
                            format!("Error: {}", error)
                        } else {
                            "ok".to_string()
                        }
                    }
                    Err(error) => format!("Error: {}", error),
                };
                let out_handle = plugin.memory_new(&status)?;
                outputs[0] = extism::Val::I64(out_handle.offset() as i64);
                Ok(())
            },
        );

        vec![f_read, f_write, f_delete]
    }
}
