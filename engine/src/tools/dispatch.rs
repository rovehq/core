use tracing::{debug, warn};

use super::registry::ToolRegistry;

impl ToolRegistry {
    /// Dispatch a tool call by name, parsing arguments from JSON.
    ///
    /// Errors are returned as strings so the LLM can inspect the failure
    /// and choose a corrective follow-up action.
    pub async fn dispatch(&self, name: &str, arguments_json: &str) -> String {
        debug!("Dispatching tool '{}' with args: {}", name, arguments_json);

        let args: serde_json::Value = match serde_json::from_str(arguments_json) {
            Ok(value) => value,
            Err(error) => return format!("ERROR: Failed to parse arguments JSON: {}", error),
        };

        if let Some(tool) = self.wasm_tools.iter().find(|tool| tool.name == name) {
            return self
                .dispatch_wasm_tool(tool.plugin_name.as_str(), name, arguments_json)
                .await;
        }

        match name {
            "web_fetch" => self.dispatch_web_fetch(&args).await,
            "read_file" => self.dispatch_read_file(&args).await,
            "write_file" => self.dispatch_write_file(&args).await,
            "patch_file" => self.dispatch_patch_file(&args).await,
            "append_to_file" => self.dispatch_append_to_file(&args).await,
            "delete_file" => self.dispatch_delete_file(&args).await,
            "list_dir" => self.dispatch_list_dir(&args).await,
            "file_exists" => self.dispatch_file_exists(&args).await,
            "glob_files" => self.dispatch_glob_files(&args).await,
            "grep_files" => self.dispatch_grep_files(&args).await,
            "run_command" => self.dispatch_run_command(&args).await,
            "capture_screen" => self.dispatch_capture_screen(&args).await,
            "browse_url" => self.dispatch_browse_url(&args).await,
            "read_page_text" => self.dispatch_read_page_text().await,
            "click_element" => self.dispatch_click_element(&args).await,
            "fill_form_field" => self.dispatch_fill_form_field(&args).await,
            _ => self.dispatch_dynamic_tool(name, args).await,
        }
    }

    async fn dispatch_wasm_tool(
        &self,
        plugin_name: &str,
        tool_name: &str,
        arguments_json: &str,
    ) -> String {
        let Some(wasm_mutex) = &self.wasm_runtime else {
            return format!("ERROR: WasmRuntime is not enabled for tool '{}'", tool_name);
        };

        let mut wasm = wasm_mutex.lock().await;
        match wasm
            .call_plugin(plugin_name, tool_name, arguments_json.as_bytes())
            .await
        {
            Ok(output) => String::from_utf8_lossy(&output).into_owned(),
            Err(error) => format!("ERROR: {}", error),
        }
    }

    async fn dispatch_read_file(&self, args: &serde_json::Value) -> String {
        let Some(fs) = &self.fs else {
            return "ERROR: read_file tool is not enabled".to_string();
        };

        let path = args
            .get("path")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        match fs.read_file(path).await {
            Ok(content) => content,
            Err(error) => format!("ERROR: {}", error),
        }
    }

    async fn dispatch_web_fetch(&self, args: &serde_json::Value) -> String {
        let url = args
            .get("url")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        if url.is_empty() {
            return "ERROR: web_fetch requires a non-empty url".to_string();
        }
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return "ERROR: web_fetch only supports http:// and https:// URLs".to_string();
        }
        let max_chars = args
            .get("max_chars")
            .and_then(|value| value.as_u64())
            .unwrap_or(20_000) as usize;

        match crate::system::knowledge::fetch_url_text(url, max_chars).await {
            Ok(result) => serde_json::json!({
                "url": result.url,
                "status": result.status,
                "content_type": result.content_type,
                "content": result.content,
            })
            .to_string(),
            Err(error) => format!("ERROR: {}", error),
        }
    }

    async fn dispatch_write_file(&self, args: &serde_json::Value) -> String {
        let Some(fs) = &self.fs else {
            return "ERROR: write_file tool is not enabled".to_string();
        };

        let path = args
            .get("path")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let content = args
            .get("content")
            .and_then(|value| value.as_str())
            .unwrap_or_default();

        match fs.write_file(path, content).await {
            Ok(message) => message,
            Err(error) => format!("ERROR: {}", error),
        }
    }

    async fn dispatch_append_to_file(&self, args: &serde_json::Value) -> String {
        let Some(fs) = &self.fs else {
            return "ERROR: append_to_file tool is not enabled".to_string();
        };
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        match fs.append_to_file(path, content).await {
            Ok(msg) => msg,
            Err(error) => format!("ERROR: {}", error),
        }
    }

    async fn dispatch_patch_file(&self, args: &serde_json::Value) -> String {
        let Some(fs) = &self.fs else {
            return "ERROR: patch_file tool is not enabled".to_string();
        };
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let old = args
            .get("old_string")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let new = args
            .get("new_string")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        match fs.patch_file(path, old, new).await {
            Ok(msg) => msg,
            Err(error) => format!("ERROR: {}", error),
        }
    }

    async fn dispatch_delete_file(&self, args: &serde_json::Value) -> String {
        let Some(fs) = &self.fs else {
            return "ERROR: delete_file tool is not enabled".to_string();
        };

        let path = args
            .get("path")
            .and_then(|value| value.as_str())
            .unwrap_or_default();

        match fs.delete_file(path).await {
            Ok(message) => message,
            Err(error) => format!("ERROR: {}", error),
        }
    }

    async fn dispatch_list_dir(&self, args: &serde_json::Value) -> String {
        let Some(fs) = &self.fs else {
            return "ERROR: list_dir tool is not enabled".to_string();
        };

        let path = args
            .get("path")
            .and_then(|value| value.as_str())
            .unwrap_or(".");
        match fs.list_dir(path).await {
            Ok(listing) => listing,
            Err(error) => format!("ERROR: {}", error),
        }
    }

    async fn dispatch_file_exists(&self, args: &serde_json::Value) -> String {
        let Some(fs) = &self.fs else {
            return "ERROR: file_exists tool is not enabled".to_string();
        };

        let path = args
            .get("path")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        match fs.file_exists(path).await {
            Ok(true) => "true".to_string(),
            Ok(false) => "false".to_string(),
            Err(error) => format!("ERROR: {}", error),
        }
    }

    async fn dispatch_glob_files(&self, args: &serde_json::Value) -> String {
        let Some(fs) = &self.fs else {
            return "ERROR: glob_files tool is not enabled".to_string();
        };

        let pattern = args
            .get("pattern")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let path = args.get("path").and_then(|value| value.as_str());
        let max_results = args
            .get("max_results")
            .and_then(|value| value.as_u64())
            .unwrap_or(200) as usize;

        match fs.glob_files(pattern, path, max_results).await {
            Ok(output) => output,
            Err(error) => format!("ERROR: {}", error),
        }
    }

    async fn dispatch_grep_files(&self, args: &serde_json::Value) -> String {
        let Some(fs) = &self.fs else {
            return "ERROR: grep_files tool is not enabled".to_string();
        };

        let pattern = args
            .get("pattern")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let path = args.get("path").and_then(|value| value.as_str());
        let file_pattern = args.get("file_pattern").and_then(|value| value.as_str());
        let max_results = args
            .get("max_results")
            .and_then(|value| value.as_u64())
            .unwrap_or(100) as usize;

        match fs
            .grep_files(pattern, path, file_pattern, max_results)
            .await
        {
            Ok(output) => output,
            Err(error) => format!("ERROR: {}", error),
        }
    }

    async fn dispatch_run_command(&self, args: &serde_json::Value) -> String {
        let Some(terminal) = &self.terminal else {
            return "ERROR: run_command tool is not enabled".to_string();
        };

        let command = args
            .get("command")
            .and_then(|value| value.as_str())
            .unwrap_or_default();

        match terminal.execute(command).await {
            Ok(output) => output,
            Err(error) => format!("ERROR: {}", error),
        }
    }

    async fn dispatch_capture_screen(&self, args: &serde_json::Value) -> String {
        let Some(vision) = &self.vision else {
            return "ERROR: capture_screen tool is not enabled".to_string();
        };

        let output_file = args
            .get("output_file")
            .and_then(|value| value.as_str())
            .unwrap_or("screenshot.png");

        match vision.capture_screen(output_file).await {
            Ok(path) => format!("Screenshot saved to {}", path.display()),
            Err(error) => format!("ERROR: {}", error),
        }
    }

    async fn dispatch_browse_url(&self, args: &serde_json::Value) -> String {
        let Some(browser) = &self.browser else {
            return "ERROR: browse_url tool is not enabled (browser control is disabled)"
                .to_string();
        };
        let url = args.get("url").and_then(|v| v.as_str()).unwrap_or_default();
        match browser.lock().await.navigate(url).await {
            Ok(result) => result,
            Err(e) => format!("ERROR: {}", e),
        }
    }

    async fn dispatch_read_page_text(&self) -> String {
        let Some(browser) = &self.browser else {
            return "ERROR: read_page_text tool is not enabled (browser control is disabled)"
                .to_string();
        };
        match browser.lock().await.page_text().await {
            Ok(text) => text,
            Err(e) => format!("ERROR: {}", e),
        }
    }

    async fn dispatch_click_element(&self, args: &serde_json::Value) -> String {
        let Some(browser) = &self.browser else {
            return "ERROR: click_element tool is not enabled (browser control is disabled)"
                .to_string();
        };
        let selector = args
            .get("selector")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        match browser.lock().await.click(selector).await {
            Ok(result) => result,
            Err(e) => format!("ERROR: {}", e),
        }
    }

    async fn dispatch_fill_form_field(&self, args: &serde_json::Value) -> String {
        let Some(browser) = &self.browser else {
            return "ERROR: fill_form_field tool is not enabled (browser control is disabled)"
                .to_string();
        };
        let selector = args
            .get("selector")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let value = args
            .get("value")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        match browser.lock().await.fill_field(selector, value).await {
            Ok(result) => result,
            Err(e) => format!("ERROR: {}", e),
        }
    }

    async fn dispatch_dynamic_tool(&self, name: &str, args: serde_json::Value) -> String {
        if let Some(mcp_tool) = self.mcp_tools.iter().find(|tool| tool.name == name) {
            return self
                .dispatch_mcp_tool(mcp_tool.server_name.as_str(), name, args)
                .await;
        }

        warn!("Unknown tool requested: {}", name);
        format!(
            "ERROR: Unknown tool '{}'. Available tools: {}",
            name,
            self.available_tool_names().join(", ")
        )
    }

    async fn dispatch_mcp_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        args: serde_json::Value,
    ) -> String {
        let Some(spawner) = &self.mcp_spawner else {
            return format!(
                "ERROR: MCP spawner not initialized for tool '{}'",
                tool_name
            );
        };

        match spawner.call_tool(server_name, tool_name, args).await {
            Ok(result) => serde_json::to_string(&result).unwrap_or_else(|_| result.to_string()),
            Err(error) => format!("ERROR: MCP tool failed: {}", error),
        }
    }
}
