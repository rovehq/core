use super::catalog::WasmToolInfo;
use super::registry::ToolRegistry;

impl ToolRegistry {
    fn query_lower(query: &str) -> String {
        query.to_lowercase()
    }

    fn should_offer_filesystem(query_lower: &str) -> bool {
        [
            "file",
            "files",
            "folder",
            "directory",
            "dir",
            "glob",
            "grep",
            "search",
            "match",
            "find",
            "read",
            "save",
            "list",
            "delete",
            "remove",
            "path",
            "source",
            "project",
            ".rs",
            "src/",
        ]
        .iter()
        .any(|keyword| query_lower.contains(keyword))
    }

    fn should_offer_terminal(query_lower: &str) -> bool {
        [
            "command",
            "terminal",
            "shell",
            "bash",
            "zsh",
            "git",
            "cargo",
            "npm",
            "pnpm",
            "yarn",
            "make",
            "build",
            "test",
            "commit",
            "status",
            "install",
            "run ",
            "execute",
            "branch",
            "repo",
            "repository",
        ]
        .iter()
        .any(|keyword| query_lower.contains(keyword))
    }

    fn should_offer_vision(query_lower: &str) -> bool {
        ["screen", "screenshot", "image", "picture", "visual"]
            .iter()
            .any(|keyword| query_lower.contains(keyword))
    }

    fn should_offer_browser(query_lower: &str) -> bool {
        [
            "browse",
            "browser",
            "webpage",
            "website",
            "url",
            "http",
            "https",
            "html",
            "click",
            "form",
            "fill",
            "navigate",
            "web page",
            "open link",
            "scrape",
        ]
        .iter()
        .any(|keyword| query_lower.contains(keyword))
    }

    fn should_offer_web(query_lower: &str) -> bool {
        [
            "web",
            "website",
            "webpage",
            "url",
            "http",
            "https",
            "search",
            "find online",
            "look up",
            "research",
            "docs",
            "documentation",
            "news",
            "article",
        ]
        .iter()
        .any(|keyword| query_lower.contains(keyword))
    }

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
                    || tool
                        .domains
                        .iter()
                        .any(|domain| query_lower.contains(domain.as_str()))
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
            out.push_str(&format!(
                "\n## {} (plugin: {})\n",
                tool.name, tool.plugin_name
            ));
            out.push_str(&tool.description);
            out.push_str(&format!("\nArguments: {}\n", tool.parameters));
        }
        out
    }

    /// Generate a system prompt describing the available tools.
    pub fn system_prompt(&self) -> String {
        self.system_prompt_for_query("")
    }

    /// Generate a system prompt describing the tools relevant to `query`.
    pub fn system_prompt_for_query(&self, query: &str) -> String {
        let query_lower = Self::query_lower(query);
        let include_all_core_tools = query_lower.is_empty();
        let include_fs = self.fs.is_some()
            && (include_all_core_tools || Self::should_offer_filesystem(&query_lower));
        let include_terminal = self.terminal.is_some()
            && (include_all_core_tools || Self::should_offer_terminal(&query_lower));
        let include_vision = self.vision.is_some()
            && (include_all_core_tools || Self::should_offer_vision(&query_lower));
        let include_web = (self.web_fetch_enabled || self.web_search_enabled)
            && (include_all_core_tools || Self::should_offer_web(&query_lower));
        let include_browser = self.browser.is_some()
            && (include_all_core_tools || Self::should_offer_browser(&query_lower));

        let has_any_tool = include_fs
            || include_terminal
            || include_vision
            || include_web
            || include_browser
            || !self.wasm_tools.is_empty()
            || !self.mcp_tools.is_empty();

        let mut parts = vec!["You are Rove, an AI agent.".to_string()];

        if has_any_tool {
            parts.push(String::new());
            parts.push("IMPORTANT RULES:".to_string());
            parts.push("1. To call a tool, your ENTIRE response must be ONLY the JSON object — nothing else. No explanation, no markdown fences, no text before or after.".to_string());
            parts.push("2. When you have the final answer (after receiving tool results), respond with plain text only — no JSON.".to_string());
            parts.push("3. Never invent tool output. Use a tool only when you need real external state, file contents, command output, or side effects. If the user asks for something you can answer from reasoning alone, answer directly without any tool call.".to_string());
            parts.push("4. If the user explicitly asks you to run a single terminal command, execute exactly that command once, then answer with its output. Do not run extra exploratory commands unless the user asked for additional investigation.".to_string());
            parts.push("5. If the user asks to read, list, write, or delete a file, use the filesystem tools and rely on their real result. Do not pre-emptively refuse or guess whether access will be allowed; the filesystem gate will enforce policy.".to_string());
            parts.push(String::new());
            parts.push("Tool call format (your entire response must be exactly this):".to_string());
            parts.push(r#"{"function": "tool_name", "arguments": {"arg1": "value1"}}"#.to_string());
            parts.push(String::new());
            parts.push("Available tools:".to_string());
        }

        if include_fs {
            parts.push(String::new());
            parts.push("## read_file".to_string());
            parts.push("Read the contents of a file.".to_string());
            parts.push(r#"Arguments: {"path": "relative/or/absolute/path"}"#.to_string());

            parts.push(String::new());
            parts.push("## write_file".to_string());
            parts.push(
                "Write content to a file (creates parent directories if needed).".to_string(),
            );
            parts.push(
                r#"Arguments: {"path": "file/path", "content": "file contents"}"#.to_string(),
            );

            parts.push(String::new());
            parts.push("## delete_file".to_string());
            parts.push("Delete a file. Use this only when the user explicitly asked to remove a file or as part of a clearly destructive action they requested.".to_string());
            parts.push(r#"Arguments: {"path": "file/path"}"#.to_string());

            parts.push(String::new());
            parts.push("## list_dir".to_string());
            parts.push(
                "List files and directories at a path. Returns entries with type, size, and name."
                    .to_string(),
            );
            parts.push(r#"Arguments: {"path": "directory/path"}"#.to_string());

            parts.push(String::new());
            parts.push("## file_exists".to_string());
            parts.push(
                r#"Check if a file or directory exists. Returns "true" or "false"."#.to_string(),
            );
            parts.push(r#"Arguments: {"path": "file/path"}"#.to_string());

            parts.push(String::new());
            parts.push("## glob_files".to_string());
            parts.push(
                "Find files matching a glob pattern inside the workspace. Useful for replacing shell `find` or broad filename scans."
                    .to_string(),
            );
            parts.push(
                r#"Arguments: {"pattern": "src/**/*.rs", "path": "optional/search/root", "max_results": 200}"#
                    .to_string(),
            );

            parts.push(String::new());
            parts.push("## grep_files".to_string());
            parts.push(
                "Search file contents with a regex and return matching lines. Useful for replacing shell `rg` when you need workspace-safe content search."
                    .to_string(),
            );
            parts.push(
                r#"Arguments: {"pattern": "workflow_runtime", "path": "optional/search/root", "file_pattern": "**/*.rs", "max_results": 100}"#
                    .to_string(),
            );

            parts.push(String::new());
            parts.push("## append_to_file".to_string());
            parts.push(
                "Append text to the end of a file. Creates the file if it does not exist. \
                 Use this instead of write_file when you only need to add to the end."
                    .to_string(),
            );
            parts.push(
                r#"Arguments: {"path": "file/path", "content": "text to append"}"#.to_string(),
            );

            parts.push(String::new());
            parts.push("## patch_file".to_string());
            parts.push(
                "Edit a file by replacing an exact string with a new string. \
                 old_string must appear exactly once — use read_file first. \
                 Prefer this over write_file for targeted edits."
                    .to_string(),
            );
            parts.push(
                r#"Arguments: {"path": "file/path", "old_string": "exact text to find", "new_string": "replacement text"}"#
                    .to_string(),
            );
        }

        if include_terminal {
            parts.push(String::new());
            parts.push("## run_command".to_string());
            parts.push(
                "Execute a shell command and return its output. Use this only for real system or repository operations, not for arithmetic, general knowledge, or questions that can be answered directly. If the user names a specific command, run that exact command and stop unless its output clearly requires a follow-up."
                    .to_string(),
            );
            parts.push(r#"Arguments: {"command": "shell command to run"}"#.to_string());
        }

        if include_vision {
            parts.push(String::new());
            parts.push("## capture_screen".to_string());
            parts.push("Capture a screenshot and save it to a file.".to_string());
            parts.push(r#"Arguments: {"output_file": "screenshot.png"}"#.to_string());
        }

        if include_web {
            if self.web_search_enabled {
                parts.push(String::new());
                parts.push("## web_search".to_string());
                parts.push(
                    "Search the web and return compact ranked results. Use this to discover relevant pages before deciding whether to fetch one."
                        .to_string(),
                );
                parts.push(
                    r#"Arguments: {"query": "search terms", "limit": 5, "provider": "searxng"}"#
                        .to_string(),
                );
            }

            if self.web_fetch_enabled {
                parts.push(String::new());
                parts.push("## web_fetch".to_string());
                parts.push(
                    "Fetch one specific web page or text resource and return readable text. Use this after you already know the URL you need."
                        .to_string(),
                );
                parts.push(
                    r#"Arguments: {"url": "https://example.com", "max_chars": 20000}"#.to_string(),
                );
            }
        }

        if include_browser {
            parts.push(String::new());
            parts.push("## browse_url".to_string());
            parts.push(
                "Navigate the browser to a URL and return the page title. \
                 Use this before read_page_text or click_element."
                    .to_string(),
            );
            parts.push(r#"Arguments: {"url": "https://example.com"}"#.to_string());

            parts.push(String::new());
            parts.push("## read_page_text".to_string());
            parts.push(
                "Get the visible text content of the current browser page (up to 8 000 chars)."
                    .to_string(),
            );
            parts.push(r#"Arguments: {}"#.to_string());

            parts.push(String::new());
            parts.push("## click_element".to_string());
            parts.push(
                "Click the first DOM element matching a CSS selector on the current page."
                    .to_string(),
            );
            parts.push(r#"Arguments: {"selector": "button.submit"}"#.to_string());

            parts.push(String::new());
            parts.push("## fill_form_field".to_string());
            parts.push(
                "Set the value of an input or textarea that matches a CSS selector.".to_string(),
            );
            parts.push(
                r##"Arguments: {"selector": "#email", "value": "user@example.com"}"##.to_string(),
            );
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
                "patch_file".to_string(),
                "append_to_file".to_string(),
                "delete_file".to_string(),
                "list_dir".to_string(),
                "file_exists".to_string(),
                "glob_files".to_string(),
                "grep_files".to_string(),
            ]);
        }
        if self.terminal.is_some() {
            names.push("run_command".to_string());
        }
        if self.vision.is_some() {
            names.push("capture_screen".to_string());
        }
        if self.browser.is_some() {
            names.extend_from_slice(&[
                "browse_url".to_string(),
                "read_page_text".to_string(),
                "click_element".to_string(),
                "fill_form_field".to_string(),
            ]);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_registry() -> ToolRegistry {
        ToolRegistry::empty()
    }

    #[test]
    fn no_tools_omits_tool_preamble() {
        let registry = empty_registry();
        let prompt = registry.system_prompt_for_query("");
        assert!(
            !prompt.contains("IMPORTANT RULES"),
            "preamble should be absent when no tools registered"
        );
        assert!(
            !prompt.contains("Available tools"),
            "tool list should be absent when no tools registered"
        );
        assert!(
            !prompt.contains("function"),
            "JSON format hint should be absent when no tools registered"
        );
        assert!(
            prompt.contains("Rove"),
            "base identity should still be present"
        );
    }

    #[test]
    fn no_tools_query_still_omits_preamble() {
        let registry = empty_registry();
        let prompt = registry.system_prompt_for_query("read the file /tmp/foo.txt");
        assert!(!prompt.contains("IMPORTANT RULES"));
        assert!(!prompt.contains("Available tools"));
    }

    #[tokio::test]
    async fn filesystem_tool_includes_preamble() {
        use crate::runtime::builtin::FilesystemTool;
        let mut registry = empty_registry();
        registry
            .register_builtin_filesystem(
                FilesystemTool::new(std::path::PathBuf::from("/tmp")).unwrap(),
            )
            .await;
        let prompt = registry.system_prompt_for_query("read the file /tmp/foo.txt");
        assert!(
            prompt.contains("IMPORTANT RULES"),
            "preamble should appear when filesystem is registered"
        );
        assert!(prompt.contains("read_file"), "read_file should be listed");
        assert!(prompt.contains("glob_files"), "glob_files should be listed");
        assert!(prompt.contains("grep_files"), "grep_files should be listed");
        assert!(prompt.contains("Available tools"));
    }

    #[test]
    fn web_tools_are_included_for_web_queries() {
        let mut registry = empty_registry();
        registry.web_fetch_enabled = true;
        registry.web_search_enabled = true;

        let prompt = registry.system_prompt_for_query("search the web for rust docs");
        assert!(prompt.contains("## web_search"));
        assert!(prompt.contains("## web_fetch"));
    }

    #[test]
    fn empty_query_with_no_tools_has_no_tool_section() {
        let registry = empty_registry();
        let prompt = registry.system_prompt();
        assert!(!prompt.contains("## read_file"));
        assert!(!prompt.contains("## run_command"));
        assert!(!prompt.contains("## browse_url"));
    }
}
