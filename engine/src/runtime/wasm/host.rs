use crate::fs_guard::FileSystemGuard;
use crate::system::memory::{MemoryIngestRequest, MemoryManager, MemoryQueryRequest};
use extism::{Function, UserData};
use reqwest::blocking::Client;
use reqwest::Method;
use sdk::manifest::PluginEntry;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use super::WasmRuntime;

#[derive(Clone)]
struct WasmHostContext {
    fs_guard: Arc<FileSystemGuard>,
    plugin: PluginEntry,
    config: Arc<crate::config::Config>,
}

#[derive(Debug, Deserialize)]
struct HostHttpFetchRequest {
    url: String,
    #[serde(default = "default_http_method")]
    method: String,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    body: Option<String>,
    timeout_secs: Option<u64>,
}

#[derive(Debug, Serialize)]
struct HostHttpFetchResponse {
    status: u16,
    headers: BTreeMap<String, String>,
    body: String,
}

#[derive(Debug, Deserialize)]
struct HostMemoryReadRequest {
    question: String,
    domain: Option<String>,
    #[serde(default)]
    explain: bool,
}

#[derive(Debug, Deserialize)]
struct HostMemoryWriteRequest {
    note: String,
    domain: Option<String>,
}

fn default_http_method() -> String {
    "GET".to_string()
}

fn max_host_payload_bytes(plugin: &PluginEntry) -> usize {
    plugin.permissions.max_file_size.unwrap_or(10 * 1024 * 1024) as usize
}

fn encode_host_payload<T: Serialize>(
    plugin: &PluginEntry,
    value: &T,
) -> Result<String, extism::Error> {
    let payload = serde_json::to_string(value).map_err(|error| {
        extism::Error::msg(format!("Failed to encode host response: {}", error))
    })?;
    let limit = max_host_payload_bytes(plugin);
    if payload.len() > limit {
        return Err(extism::Error::msg(format!(
            "Host response exceeded {} bytes",
            limit
        )));
    }
    Ok(payload)
}

fn run_memory_future<F, T>(future: F) -> Result<T, extism::Error>
where
    F: Future<Output = anyhow::Result<T>> + Send + 'static,
    T: Send + 'static,
{
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("Failed to create memory runtime: {}", error))?;
        runtime.block_on(future).map_err(|error| error.to_string())
    })
    .join()
    .map_err(|_| extism::Error::msg("Memory runtime thread panicked"))?
    .map_err(extism::Error::msg)
}

fn handle_memory_read(
    context: &WasmHostContext,
    request: HostMemoryReadRequest,
) -> Result<String, extism::Error> {
    if !context.plugin.can_read_memory() {
        return Err(extism::Error::msg("Plugin is not allowed to read memory"));
    }

    let config = (*context.config).clone();
    let response = run_memory_future(async move {
        MemoryManager::new(config)
            .query(MemoryQueryRequest {
                question: request.question,
                explain: request.explain,
                domain: request.domain,
            })
            .await
    })?;

    encode_host_payload(&context.plugin, &response)
}

fn handle_memory_write(
    context: &WasmHostContext,
    request: HostMemoryWriteRequest,
) -> Result<String, extism::Error> {
    if !context.plugin.can_write_memory() {
        return Err(extism::Error::msg("Plugin is not allowed to write memory"));
    }

    let config = (*context.config).clone();
    let response = run_memory_future(async move {
        MemoryManager::new(config)
            .ingest_note(MemoryIngestRequest {
                note: request.note,
                domain: request.domain,
            })
            .await
    })?;

    encode_host_payload(&context.plugin, &response)
}

impl WasmRuntime {
    pub(super) fn create_host_functions(&self, plugin_entry: &PluginEntry) -> Vec<Function> {
        let host_context = Arc::new(WasmHostContext {
            fs_guard: Arc::clone(&self.fs_guard),
            plugin: plugin_entry.clone(),
            config: Arc::clone(&self.config),
        });
        let f_read = Function::new(
            "host_read_file",
            [extism::ValType::I64],
            [extism::ValType::I64],
            UserData::new(Arc::clone(&host_context)),
            |plugin: &mut extism::CurrentPlugin,
             inputs: &[extism::Val],
             outputs: &mut [extism::Val],
             user_data: UserData<Arc<WasmHostContext>>|
             -> Result<(), extism::Error> {
                let path_offset = inputs[0].unwrap_i64() as u64;
                let path_handle = plugin
                    .memory_handle(path_offset)
                    .ok_or_else(|| extism::Error::msg("Invalid path offset"))?;
                let path_str = plugin.memory_str(path_handle)?.to_string();
                let binding = user_data.get()?;
                let context = binding.lock().map_err(|error| {
                    extism::Error::msg(format!("WASM host context lock poisoned: {}", error))
                })?;
                let content = match context
                    .fs_guard
                    .validate_path(std::path::Path::new(&path_str))
                {
                    Ok(path) => std::fs::read_to_string(path)
                        .unwrap_or_else(|error| format!("Error: {}", error)),
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
            UserData::new(Arc::clone(&host_context)),
            |plugin: &mut extism::CurrentPlugin,
             inputs: &[extism::Val],
             outputs: &mut [extism::Val],
             user_data: UserData<Arc<WasmHostContext>>|
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
                let context = binding.lock().map_err(|error| {
                    extism::Error::msg(format!("WASM host context lock poisoned: {}", error))
                })?;
                let status = match context
                    .fs_guard
                    .validate_path(std::path::Path::new(&path_str))
                {
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
            UserData::new(Arc::clone(&host_context)),
            |plugin: &mut extism::CurrentPlugin,
             inputs: &[extism::Val],
             outputs: &mut [extism::Val],
             user_data: UserData<Arc<WasmHostContext>>|
             -> Result<(), extism::Error> {
                let path_offset = inputs[0].unwrap_i64() as u64;
                let path_handle = plugin
                    .memory_handle(path_offset)
                    .ok_or_else(|| extism::Error::msg("Invalid path offset"))?;
                let path_str = plugin.memory_str(path_handle)?.to_string();
                let binding = user_data.get()?;
                let context = binding.lock().map_err(|error| {
                    extism::Error::msg(format!("WASM host context lock poisoned: {}", error))
                })?;
                let status = match context
                    .fs_guard
                    .validate_path(std::path::Path::new(&path_str))
                {
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

        let f_http_fetch = Function::new(
            "host_http_fetch",
            [extism::ValType::I64],
            [extism::ValType::I64],
            UserData::new(Arc::clone(&host_context)),
            |plugin: &mut extism::CurrentPlugin,
             inputs: &[extism::Val],
             outputs: &mut [extism::Val],
             user_data: UserData<Arc<WasmHostContext>>|
             -> Result<(), extism::Error> {
                let request_offset = inputs[0].unwrap_i64() as u64;
                let request_handle = plugin
                    .memory_handle(request_offset)
                    .ok_or_else(|| extism::Error::msg("Invalid request offset"))?;
                let request_str = plugin.memory_str(request_handle)?.to_string();
                let request: HostHttpFetchRequest =
                    serde_json::from_str(&request_str).map_err(|error| {
                        extism::Error::msg(format!("Invalid fetch request JSON: {}", error))
                    })?;

                let url = reqwest::Url::parse(&request.url)
                    .map_err(|error| extism::Error::msg(format!("Invalid fetch URL: {}", error)))?;
                if !matches!(url.scheme(), "http" | "https") {
                    return Err(extism::Error::msg(
                        "host_http_fetch only supports http/https",
                    ));
                }

                let binding = user_data.get()?;
                let context = binding.lock().map_err(|error| {
                    extism::Error::msg(format!("WASM host context lock poisoned: {}", error))
                })?;
                let host = url
                    .host_str()
                    .ok_or_else(|| extism::Error::msg("Fetch URL is missing a host"))?;
                if !context.plugin.is_network_host_allowed(host) {
                    return Err(extism::Error::msg(format!(
                        "Network access to '{}' is not allowed",
                        host
                    )));
                }

                let timeout = request.timeout_secs.unwrap_or(30).clamp(1, 30);
                let client = Client::builder()
                    .timeout(Duration::from_secs(timeout))
                    .build()
                    .map_err(|error| {
                        extism::Error::msg(format!("Failed to build HTTP client: {}", error))
                    })?;
                let method =
                    Method::from_bytes(request.method.trim().as_bytes()).map_err(|error| {
                        extism::Error::msg(format!(
                            "Invalid HTTP method '{}': {}",
                            request.method, error
                        ))
                    })?;
                let mut builder = client.request(method, url);
                for (name, value) in &request.headers {
                    builder = builder.header(name, value);
                }
                if let Some(body) = request.body {
                    builder = builder.body(body);
                }

                let response = builder.send().map_err(|error| {
                    extism::Error::msg(format!("HTTP request failed: {}", error))
                })?;
                let status = response.status().as_u16();
                let headers = response
                    .headers()
                    .iter()
                    .filter_map(|(name, value)| {
                        value
                            .to_str()
                            .ok()
                            .map(|value| (name.to_string(), value.to_string()))
                    })
                    .collect::<BTreeMap<_, _>>();
                let body = response.text().map_err(|error| {
                    extism::Error::msg(format!("Failed to read HTTP response: {}", error))
                })?;
                let max_response_bytes = max_host_payload_bytes(&context.plugin);
                if body.len() > max_response_bytes {
                    return Err(extism::Error::msg(format!(
                        "HTTP response exceeded {} bytes",
                        max_response_bytes
                    )));
                }

                let payload = encode_host_payload(
                    &context.plugin,
                    &HostHttpFetchResponse {
                        status,
                        headers,
                        body,
                    },
                )?;
                let out_handle = plugin.memory_new(&payload)?;
                outputs[0] = extism::Val::I64(out_handle.offset() as i64);
                Ok(())
            },
        );

        let f_memory_read = Function::new(
            "host_memory_read",
            [extism::ValType::I64],
            [extism::ValType::I64],
            UserData::new(Arc::clone(&host_context)),
            |plugin: &mut extism::CurrentPlugin,
             inputs: &[extism::Val],
             outputs: &mut [extism::Val],
             user_data: UserData<Arc<WasmHostContext>>|
             -> Result<(), extism::Error> {
                let request_offset = inputs[0].unwrap_i64() as u64;
                let request_handle = plugin
                    .memory_handle(request_offset)
                    .ok_or_else(|| extism::Error::msg("Invalid request offset"))?;
                let request: HostMemoryReadRequest = serde_json::from_str(
                    plugin.memory_str(request_handle)?.as_ref(),
                )
                .map_err(|error| {
                    extism::Error::msg(format!("Invalid memory read request JSON: {}", error))
                })?;
                let binding = user_data.get()?;
                let context = binding.lock().map_err(|error| {
                    extism::Error::msg(format!("WASM host context lock poisoned: {}", error))
                })?;
                let payload = handle_memory_read(&context, request)?;
                let out_handle = plugin.memory_new(&payload)?;
                outputs[0] = extism::Val::I64(out_handle.offset() as i64);
                Ok(())
            },
        );

        let f_memory_write = Function::new(
            "host_memory_write",
            [extism::ValType::I64],
            [extism::ValType::I64],
            UserData::new(host_context),
            |plugin: &mut extism::CurrentPlugin,
             inputs: &[extism::Val],
             outputs: &mut [extism::Val],
             user_data: UserData<Arc<WasmHostContext>>|
             -> Result<(), extism::Error> {
                let request_offset = inputs[0].unwrap_i64() as u64;
                let request_handle = plugin
                    .memory_handle(request_offset)
                    .ok_or_else(|| extism::Error::msg("Invalid request offset"))?;
                let request: HostMemoryWriteRequest = serde_json::from_str(
                    plugin.memory_str(request_handle)?.as_ref(),
                )
                .map_err(|error| {
                    extism::Error::msg(format!("Invalid memory write request JSON: {}", error))
                })?;
                let binding = user_data.get()?;
                let context = binding.lock().map_err(|error| {
                    extism::Error::msg(format!("WASM host context lock poisoned: {}", error))
                })?;
                let payload = handle_memory_write(&context, request)?;
                let out_handle = plugin.memory_new(&payload)?;
                outputs[0] = extism::Val::I64(out_handle.offset() as i64);
                Ok(())
            },
        );

        vec![
            f_read,
            f_write,
            f_delete,
            f_http_fetch,
            f_memory_read,
            f_memory_write,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::{
        handle_memory_read, handle_memory_write, HostMemoryReadRequest, HostMemoryWriteRequest,
        WasmHostContext,
    };
    use crate::config::Config;
    use crate::fs_guard::FileSystemGuard;
    use crate::system::memory::MemoryQueryResponse;
    use sdk::manifest::PluginEntry;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn test_context(memory_read: bool, memory_write: bool) -> (WasmHostContext, TempDir, TempDir) {
        let workspace = TempDir::new().expect("workspace");
        let data_dir = TempDir::new().expect("data");
        let mut config = Config::default();
        config.core.workspace = workspace.path().to_path_buf();
        config.core.data_dir = data_dir.path().to_path_buf();

        let mut plugin = PluginEntry::default();
        plugin.name = "memory-plugin".to_string();
        plugin.permissions.memory_read = memory_read;
        plugin.permissions.memory_write = memory_write;

        (
            WasmHostContext {
                fs_guard: Arc::new(
                    FileSystemGuard::new(workspace.path().to_path_buf()).expect("fs guard"),
                ),
                plugin,
                config: Arc::new(config),
            },
            workspace,
            data_dir,
        )
    }

    #[test]
    fn memory_write_requires_permission() {
        let (context, _workspace, _data_dir) = test_context(false, false);
        let error = handle_memory_write(
            &context,
            HostMemoryWriteRequest {
                note: "remember tenant alpha uses staging".to_string(),
                domain: Some("general".to_string()),
            },
        )
        .expect_err("write should be denied");
        assert!(error.to_string().contains("not allowed to write memory"));
    }

    #[test]
    fn memory_read_requires_permission() {
        let (context, _workspace, _data_dir) = test_context(false, true);
        let error = handle_memory_read(
            &context,
            HostMemoryReadRequest {
                question: "tenant alpha".to_string(),
                domain: Some("general".to_string()),
                explain: false,
            },
        )
        .expect_err("read should be denied");
        assert!(error.to_string().contains("not allowed to read memory"));
    }

    #[test]
    fn memory_host_round_trip_reads_written_note() {
        let (context, _workspace, _data_dir) = test_context(true, true);

        let write_payload = handle_memory_write(
            &context,
            HostMemoryWriteRequest {
                note: "remember that tenant alpha uses the staging API".to_string(),
                domain: Some("general".to_string()),
            },
        )
        .expect("write payload");
        assert!(write_payload.contains("tenant alpha"));

        let read_payload = handle_memory_read(
            &context,
            HostMemoryReadRequest {
                question: "what do we remember about tenant alpha".to_string(),
                domain: Some("general".to_string()),
                explain: true,
            },
        )
        .expect("read payload");
        let response: MemoryQueryResponse =
            serde_json::from_str(&read_payload).expect("query response json");
        assert!(
            response
                .facts
                .iter()
                .any(|hit| hit.content.to_ascii_lowercase().contains("tenant alpha")),
            "expected a remembered fact about tenant alpha, got {:?}",
            response.facts
        );
        assert!(response.explain.is_some());
    }
}
