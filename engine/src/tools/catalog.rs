/// Map a plugin name to domain keyword tags used for dynamic tool filtering.
///
/// Examples:
/// - `"fs-editor"` -> `["filesystem", "file", "edit", "read", "write"]`
/// - `"git-tools"` -> `["git", "version", "commit"]`
pub fn derive_domains_from_name(name: &str) -> Vec<String> {
    let n = name.to_lowercase();
    let mut domains = Vec::new();

    if n.contains("fs") || n.contains("file") || n.contains("dir") {
        domains.extend_from_slice(&[
            "filesystem".to_string(),
            "file".to_string(),
            "read".to_string(),
            "write".to_string(),
            "edit".to_string(),
        ]);
    }
    if n.contains("git") {
        domains.extend_from_slice(&[
            "git".to_string(),
            "version".to_string(),
            "commit".to_string(),
        ]);
    }
    if n.contains("web") || n.contains("http") || n.contains("fetch") {
        domains.extend_from_slice(&[
            "web".to_string(),
            "http".to_string(),
            "fetch".to_string(),
            "scrape".to_string(),
        ]);
    }
    if n.contains("db") || n.contains("sql") || n.contains("data") {
        domains.extend_from_slice(&[
            "database".to_string(),
            "sql".to_string(),
            "query".to_string(),
        ]);
    }
    if n.contains("image") || n.contains("vision") || n.contains("screen") {
        domains.extend_from_slice(&["vision".to_string(), "image".to_string()]);
    }

    for word in n.split('-').map(str::to_string) {
        if word.len() > 2 && !domains.contains(&word) {
            domains.push(word);
        }
    }

    domains
}

/// Information about an available WASM tool.
#[derive(Clone)]
pub struct WasmToolInfo {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub plugin_name: String,
    /// Domain tags for dynamic tool filtering (for example `["filesystem", "edit"]`).
    pub domains: Vec<String>,
}

/// Information about an available MCP tool.
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub server_name: String,
    pub domains: Vec<String>,
}
