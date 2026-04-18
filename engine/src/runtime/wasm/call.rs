use std::collections::BTreeSet;

use sdk::errors::EngineError;
use serde_json::Value;

use crate::security::secrets::scrub_text_with_values;

use super::{WasmRuntime, MAX_CRASH_RESTARTS};

fn collect_hosts(value: &Value, hosts: &mut BTreeSet<String>) {
    match value {
        Value::String(text) => {
            if let Ok(url) = reqwest::Url::parse(text) {
                if let Some(host) = url.host_str() {
                    hosts.insert(host.to_string());
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_hosts(item, hosts);
            }
        }
        Value::Object(map) => {
            for item in map.values() {
                collect_hosts(item, hosts);
            }
        }
        _ => {}
    }
}

fn collect_hosts_from_text(text: &str, hosts: &mut BTreeSet<String>) {
    for token in text.split_whitespace() {
        let candidate = token.trim_matches(|ch: char| {
            matches!(
                ch,
                '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
            )
        });
        if let Ok(url) = reqwest::Url::parse(candidate) {
            if let Some(host) = url.host_str() {
                hosts.insert(host.to_string());
            }
        }
    }
}

fn collect_placeholders(text: &str) -> Vec<String> {
    let mut placeholders = Vec::new();
    let mut cursor = 0usize;
    while let Some(open_rel) = text[cursor..].find('{') {
        let open = cursor + open_rel;
        let Some(close_rel) = text[open + 1..].find('}') else {
            break;
        };
        let close = open + 1 + close_rel;
        let candidate = &text[open + 1..close];
        if !candidate.is_empty()
            && candidate
                .chars()
                .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
        {
            placeholders.push(candidate.to_string());
        }
        cursor = close + 1;
    }
    placeholders
}

fn collect_value_placeholders(value: &Value, placeholders: &mut BTreeSet<String>) {
    match value {
        Value::String(text) => {
            for placeholder in collect_placeholders(text) {
                placeholders.insert(placeholder);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_value_placeholders(item, placeholders);
            }
        }
        Value::Object(map) => {
            for item in map.values() {
                collect_value_placeholders(item, placeholders);
            }
        }
        _ => {}
    }
}

fn replace_placeholders_in_value(value: &mut Value, replacements: &[(String, String)]) {
    match value {
        Value::String(text) => {
            let mut updated = text.clone();
            for (placeholder, secret) in replacements {
                updated = updated.replace(&format!("{{{placeholder}}}"), secret);
            }
            *text = updated;
        }
        Value::Array(items) => {
            for item in items {
                replace_placeholders_in_value(item, replacements);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                replace_placeholders_in_value(item, replacements);
            }
        }
        _ => {}
    }
}

impl WasmRuntime {
    pub async fn call_plugin(
        &mut self,
        name: &str,
        function: &str,
        input: &[u8],
    ) -> Result<Vec<u8>, EngineError> {
        tracing::debug!("Calling plugin '{}' function '{}'", name, function);

        self.validate_call_permissions(name, input)?;
        let (prepared_input, resolved_secrets) = self.prepare_call_input(name, input).await?;

        if !self.plugins.contains_key(name) {
            tracing::debug!(
                "Plugin '{}' not loaded yet; loading lazily on first call",
                name
            );
            self.load_plugin(name).await?;
        }

        let metadata = self.plugins.get_mut(name).ok_or_else(|| {
            tracing::error!("Plugin '{}' not loaded", name);
            EngineError::PluginNotLoaded(name.to_string())
        })?;

        if metadata.crash_count >= MAX_CRASH_RESTARTS {
            tracing::error!(
                "Plugin '{}' has crashed {} times, refusing to call",
                name,
                metadata.crash_count
            );
            return Err(EngineError::Plugin(format!(
                "Plugin '{}' has crashed too many times ({} crashes)",
                name, metadata.crash_count
            )));
        }

        let result = metadata
            .plugin
            .call::<&[u8], Vec<u8>>(function, &prepared_input)
            .map_err(|error| {
                let error_text = scrub_text_with_values(
                    &format!("Plugin call failed: {}", error),
                    &resolved_secrets,
                );
                tracing::error!(
                    "Plugin '{}' function '{}' failed: {}",
                    name,
                    function,
                    error_text
                );
                EngineError::Plugin(error_text)
            });

        match result {
            Ok(output) => {
                if metadata.crash_count > 0 {
                    tracing::info!(
                        "Plugin '{}' recovered after {} crashes",
                        name,
                        metadata.crash_count
                    );
                    metadata.crash_count = 0;
                }
                Ok(self.scrub_plugin_output(output, &resolved_secrets))
            }
            Err(error) => {
                self.handle_plugin_crash(name, &error).await?;
                tracing::info!(
                    "Retrying plugin '{}' function '{}' after restart",
                    name,
                    function
                );

                let metadata = self.plugins.get_mut(name).ok_or_else(|| {
                    EngineError::Plugin(format!("Plugin '{}' disappeared after restart", name))
                })?;

                metadata
                    .plugin
                    .call::<&[u8], Vec<u8>>(function, &prepared_input)
                    .map_err(|retry_error| {
                        let error_text = scrub_text_with_values(
                            &format!("Plugin call failed after restart: {}", retry_error),
                            &resolved_secrets,
                        );
                        tracing::error!(
                            "Plugin '{}' function '{}' failed again after restart: {}",
                            name,
                            function,
                            error_text
                        );
                        EngineError::Plugin(error_text)
                    })
                    .map(|output| self.scrub_plugin_output(output, &resolved_secrets))
            }
        }
    }

    async fn prepare_call_input(
        &self,
        name: &str,
        input: &[u8],
    ) -> Result<(Vec<u8>, Vec<String>), EngineError> {
        let plugin_entry = self
            .manifest
            .get_plugin(name)
            .ok_or_else(|| EngineError::PluginNotInManifest(name.to_string()))?;

        if let Ok(mut input_json) = serde_json::from_slice::<Value>(input) {
            let mut placeholders = BTreeSet::new();
            collect_value_placeholders(&input_json, &mut placeholders);
            if placeholders.is_empty() {
                return Ok((input.to_vec(), Vec::new()));
            }

            let mut hosts = BTreeSet::new();
            collect_hosts(&input_json, &mut hosts);
            let (replacements, resolved_values) = self
                .resolve_secret_replacements(name, plugin_entry, placeholders, hosts)
                .await?;

            replace_placeholders_in_value(&mut input_json, &replacements);
            let encoded = serde_json::to_vec(&input_json).map_err(|error| {
                EngineError::Plugin(format!("Failed to encode plugin input: {}", error))
            })?;
            return Ok((encoded, resolved_values));
        }

        let input_text = match std::str::from_utf8(input) {
            Ok(text) => text,
            Err(_) => return Ok((input.to_vec(), Vec::new())),
        };

        let placeholders = collect_placeholders(input_text)
            .into_iter()
            .collect::<BTreeSet<_>>();
        if placeholders.is_empty() {
            return Ok((input.to_vec(), Vec::new()));
        }

        let mut hosts = BTreeSet::new();
        collect_hosts_from_text(input_text, &mut hosts);
        let (replacements, resolved_values) = self
            .resolve_secret_replacements(name, plugin_entry, placeholders, hosts)
            .await?;

        let mut updated = input_text.to_string();
        for (placeholder, value) in &replacements {
            updated = updated.replace(&format!("{{{placeholder}}}"), value);
        }

        Ok((updated.into_bytes(), resolved_values))
    }

    async fn resolve_secret_replacements(
        &self,
        name: &str,
        plugin_entry: &sdk::PluginEntry,
        placeholders: BTreeSet<String>,
        hosts: BTreeSet<String>,
    ) -> Result<(Vec<(String, String)>, Vec<String>), EngineError> {
        if hosts.is_empty() {
            return Err(EngineError::Plugin(format!(
                "Plugin '{}' requested secret placeholder injection without a declared destination host in input",
                name
            )));
        }

        for host in &hosts {
            if !plugin_entry.is_secret_host_allowed(host) {
                return Err(EngineError::Plugin(format!(
                    "Plugin '{}' is not allowed to inject secrets for host '{}'",
                    name, host
                )));
            }
        }

        let mut replacements = Vec::new();
        let mut resolved_values = Vec::new();
        for placeholder in placeholders {
            if !plugin_entry.is_secret_declared(&placeholder) {
                return Err(EngineError::Plugin(format!(
                    "Plugin '{}' requested undeclared secret '{}'",
                    name, placeholder
                )));
            }

            let value = self
                .secret_manager
                .lookup_secret(&placeholder)
                .await
                .map(|(value, _)| value)
                .ok_or_else(|| {
                    EngineError::Plugin(format!(
                        "Plugin '{}' requires secret '{}' but it is not configured",
                        name, placeholder
                    ))
                })?;

            resolved_values.push(value.clone());
            replacements.push((placeholder, value));
        }

        Ok((replacements, resolved_values))
    }

    fn scrub_plugin_output(&self, output: Vec<u8>, resolved_secrets: &[String]) -> Vec<u8> {
        let text = String::from_utf8_lossy(&output).into_owned();
        scrub_text_with_values(&text, resolved_secrets).into_bytes()
    }

    fn validate_call_permissions(&self, name: &str, input: &[u8]) -> Result<(), EngineError> {
        let plugin_entry = self
            .manifest
            .get_plugin(name)
            .ok_or_else(|| EngineError::PluginNotInManifest(name.to_string()))?;

        if let Ok(input_json) = serde_json::from_slice::<serde_json::Value>(input) {
            if let Some(path) = input_json.get("path").and_then(|value| value.as_str()) {
                if !plugin_entry.is_path_allowed(path) {
                    tracing::warn!(
                        plugin = name,
                        path = path,
                        "Plugin attempted to access denied path"
                    );
                    return Err(EngineError::PathDenied(std::path::PathBuf::from(path)));
                }
            }

            if let Some(command) = input_json.get("command").and_then(|value| value.as_str()) {
                if !plugin_entry.is_command_allowed(command) {
                    tracing::warn!(
                        plugin = name,
                        command = command,
                        "Plugin attempted to execute denied command"
                    );
                    return Err(EngineError::CommandNotAllowed(command.to_string()));
                }
            }

            if let Some(size) = input_json.get("size").and_then(|value| value.as_u64()) {
                if let Some(max_size) = plugin_entry.permissions.max_file_size {
                    if size > max_size {
                        tracing::warn!(
                            plugin = name,
                            size = size,
                            max_size = max_size,
                            "Plugin attempted to exceed max file size"
                        );
                        return Err(EngineError::Plugin(format!(
                            "File size {} exceeds maximum allowed size {}",
                            size, max_size
                        )));
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        collect_hosts, collect_hosts_from_text, collect_placeholders, collect_value_placeholders,
        replace_placeholders_in_value,
    };
    use serde_json::json;
    use std::collections::BTreeSet;

    #[test]
    fn collects_secret_placeholders_from_nested_values() {
        let value = json!({
            "url": "https://api.openai.com/v1/responses",
            "headers": {
                "Authorization": "Bearer {OPENAI_API_KEY}"
            },
            "nested": ["{SECONDARY_KEY}", "plain"]
        });

        let mut placeholders = BTreeSet::new();
        collect_value_placeholders(&value, &mut placeholders);
        assert_eq!(
            placeholders.into_iter().collect::<Vec<_>>(),
            vec!["OPENAI_API_KEY".to_string(), "SECONDARY_KEY".to_string()]
        );
    }

    #[test]
    fn replaces_placeholders_in_nested_values() {
        let mut value = json!({
            "headers": {
                "Authorization": "Bearer {OPENAI_API_KEY}"
            },
            "query": "{OPENAI_API_KEY}"
        });

        replace_placeholders_in_value(
            &mut value,
            &[("OPENAI_API_KEY".to_string(), "sk-live-secret".to_string())],
        );

        assert_eq!(value["headers"]["Authorization"], "Bearer sk-live-secret");
        assert_eq!(value["query"], "sk-live-secret");
    }

    #[test]
    fn collects_hosts_from_nested_urls() {
        let value = json!({
            "url": "https://api.openai.com/v1/responses",
            "items": [
                {"endpoint": "https://example.com/path"}
            ]
        });

        let mut hosts = BTreeSet::new();
        collect_hosts(&value, &mut hosts);
        assert!(hosts.contains("api.openai.com"));
        assert!(hosts.contains("example.com"));
    }

    #[test]
    fn collects_hosts_from_plain_text_urls() {
        let mut hosts = BTreeSet::new();
        collect_hosts_from_text(
            "POST https://api.openai.com/v1/responses with callback https://example.com/hook",
            &mut hosts,
        );

        assert!(hosts.contains("api.openai.com"));
        assert!(hosts.contains("example.com"));
    }

    #[test]
    fn placeholder_parser_ignores_non_secret_braces() {
        let placeholders = collect_placeholders("hello {not-secret} and {UPPER_SECRET_1}");
        assert_eq!(placeholders, vec!["UPPER_SECRET_1".to_string()]);
    }
}
