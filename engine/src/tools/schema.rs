use super::catalog::WasmToolInfo;
use super::registry::ToolRegistry;

impl ToolRegistry {
    /// Return WASM tools whose domain tags overlap with keywords in `query`.
    ///
    /// Falls back to all WASM tools when the query matches nothing or
    /// when no tools have domain tags.
    pub fn tools_for_domain<'a>(&'a self, query: &str) -> Vec<&'a WasmToolInfo> {
        let query_lower = query.to_lowercase();

        let matched: Vec<&WasmToolInfo> = self
            .wasm_tools
            .iter()
            .filter(|tool| {
                tool.domains.is_empty()
                    || tool.domains.iter().any(|domain| query_lower.contains(domain.as_str()))
                    || query_lower.contains(&tool.name.to_lowercase())
            })
            .collect();

        if matched.is_empty() {
            self.wasm_tools.iter().collect()
        } else {
            matched
        }
    }

    /// Generate a tool schema block for injection into the system prompt.
    pub fn tool_schemas_for_prompt(&self, query: &str) -> String {
        let tools = self.tools_for_domain(query);
        if tools.is_empty() {
            return String::new();
        }

        let mut out = String::new();
        for tool in tools {
            out.push_str(&format!("\n## {} (plugin: {})\n", tool.name, tool.plugin_name));
            out.push_str(&tool.description);
            out.push_str(&format!("\nArguments: {}\n", tool.parameters));
        }
        out
    }

    /// Generate a system prompt describing the available tools.
    pub fn system_prompt(&self) -> String {
        let mut parts = vec![
            "You are Rove, an AI agent that can use tools to accomplish tasks.".to_string(),
            String::new(),
            "IMPORTANT RULES:".to_string(),
            "1. To call a tool, your ENTIRE response must be ONLY the JSON object — nothing else. No explanation, no markdown fences, no text before or after.".to_string(),
            "2. When you have the final answer (after receiving tool results), respond with plain text only — no JSON.".to_string(),
            "3. Never guess or hallucinate tool output. Always call the tool and wait for the real result.".to_string(),
            String::new(),
            "Tool call format (your entire response must be exactly this):".to_string(),
            r#"{"function": "tool_name", "arguments": {"arg1": "value1"}}"#.to_string(),
            String::new(),
            "Available tools:".to_string(),
        ];

        if self.fs.is_some() {
            parts.push(String::new());
            parts.push("## read_file".to_string());
            parts.push("Read the contents of a file.".to_string());
            parts.push(r#"Arguments: {"path": "relative/or/absolute/path"}"#.to_string());

            parts.push(String::new());
            parts.push("## write_file".to_string());
            parts.push("Write content to a file (creates parent directories if needed).".to_string());
            parts.push(r#"Arguments: {"path": "file/path", "content": "file contents"}"#.to_string());

            parts.push(String::new());
            parts.push("## list_dir".to_string());
            parts.push(
                "List files and directories at a path. Returns entries with type, size, and name."
                    .to_string(),
            );
            parts.push(r#"Arguments: {"path": "directory/path"}"#.to_string());

            parts.push(String::new());
            parts.push("## file_exists".to_string());
            parts.push(r#"Check if a file or directory exists. Returns "true" or "false"."#.to_string());
            parts.push(r#"Arguments: {"path": "file/path"}"#.to_string());
        }

        if self.terminal.is_some() {
            parts.push(String::new());
            parts.push("## run_command".to_string());
            parts.push("Execute a shell command and return its output.".to_string());
            parts.push(r#"Arguments: {"command": "shell command to run"}"#.to_string());
        }

        if self.vision.is_some() {
            parts.push(String::new());
            parts.push("## capture_screen".to_string());
            parts.push("Capture a screenshot and save it to a file.".to_string());
            parts.push(r#"Arguments: {"output_file": "screenshot.png"}"#.to_string());
        }

        for tool in &self.wasm_tools {
            parts.push(String::new());
            parts.push(format!("## {}", tool.name));
            parts.push(tool.description.clone());
            parts.push(format!("Arguments: {}", tool.parameters));
        }

        for tool in &self.mcp_tools {
            parts.push(String::new());
            parts.push(format!("## {} (MCP: {})", tool.name, tool.server_name));
            parts.push(tool.description.clone());
            parts.push(format!("Arguments: {}", tool.parameters));
        }

        parts.join("\n")
    }

    pub(crate) fn available_tool_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        if self.fs.is_some() {
            names.extend_from_slice(&[
                "read_file".to_string(),
                "write_file".to_string(),
                "list_dir".to_string(),
                "file_exists".to_string(),
            ]);
        }
        if self.terminal.is_some() {
            names.push("run_command".to_string());
        }
        if self.vision.is_some() {
            names.push("capture_screen".to_string());
        }
        for tool in &self.wasm_tools {
            names.push(tool.name.clone());
        }
        for tool in &self.mcp_tools {
            names.push(tool.name.clone());
        }
        names
    }
}
