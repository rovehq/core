use super::ToolCall;

/// Helper function to parse tool calls from string content.
///
/// Handles multiple LLM output formats:
/// 1. Raw JSON: `{"function": "...", "arguments": {...}}`
/// 2. Fenced JSON: ` ```json\n{...}\n``` `
/// 3. `<tool_call>name({...})</tool_call>` XML markers
/// 4. JSON embedded in prose
pub fn parse_tool_calls(content: &str) -> Option<ToolCall> {
    let trimmed = content.trim();

    if let Some(tool_call) = try_parse_function_json(trimmed) {
        return Some(tool_call);
    }

    if let Some(inner) = extract_fenced_json(trimmed) {
        if let Some(tool_call) = try_parse_function_json(inner.trim()) {
            return Some(tool_call);
        }
    }

    if let Some(start) = trimmed.find("<tool_call>") {
        if let Some(end) = trimmed.find("</tool_call>") {
            let tool_content = &trimmed[start + 11..end];
            if let Some(paren_pos) = tool_content.find('(') {
                let tool_name = &tool_content[..paren_pos];
                let args_end = tool_content.rfind(')').unwrap_or(tool_content.len());
                let arguments = &tool_content[paren_pos + 1..args_end];

                return Some(ToolCall::new(
                    format!("call_{}", uuid::Uuid::new_v4()),
                    tool_name.trim(),
                    arguments,
                ));
            }
        }
    }

    if let Some(pos) = trimmed.find("{\"function\"") {
        let candidate = &trimmed[pos..];
        if let Some(json_str) = extract_balanced_json(candidate) {
            if let Some(tool_call) = try_parse_function_json(json_str) {
                return Some(tool_call);
            }
        }
    }

    None
}

fn try_parse_function_json(content: &str) -> Option<ToolCall> {
    let json: serde_json::Value = serde_json::from_str(content).ok()?;
    let function = json.get("function")?.as_str()?;
    let arguments = json.get("arguments")?;
    Some(ToolCall::new(
        format!("call_{}", uuid::Uuid::new_v4()),
        function,
        arguments.to_string(),
    ))
}

fn extract_fenced_json(content: &str) -> Option<&str> {
    let fence_start = content.find("```")?;
    let after_opening = &content[fence_start + 3..];
    let body_start_rel = after_opening.find('\n')? + 1;
    let body_start = fence_start + 3 + body_start_rel;
    let closing = content[body_start..].find("```")?;
    let body_end = body_start + closing;

    if body_start >= body_end {
        return None;
    }

    Some(&content[body_start..body_end])
}

fn extract_balanced_json(content: &str) -> Option<&str> {
    if !content.starts_with('{') {
        return None;
    }

    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape_next = false;

    for (index, ch) in content.char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }

        match ch {
            '\\' if in_string => escape_next = true,
            '"' => in_string = !in_string,
            '{' if !in_string => depth += 1,
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some(&content[..=index]);
                }
            }
            _ => {}
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::parse_tool_calls;

    #[test]
    fn test_parse_tool_calls_json_format() {
        let content = r#"{"function": "read_file", "arguments": {"path": "test.txt"}}"#;
        let tool_call = parse_tool_calls(content);

        assert!(tool_call.is_some());
        let tool_call = tool_call.unwrap();
        assert_eq!(tool_call.name, "read_file");
        assert!(tool_call.arguments.contains("path"));
    }

    #[test]
    fn test_parse_tool_calls_marker_format() {
        let content = r#"<tool_call>read_file({"path": "test.txt"})</tool_call>"#;
        let tool_call = parse_tool_calls(content);

        assert!(tool_call.is_some());
        let tool_call = tool_call.unwrap();
        assert_eq!(tool_call.name, "read_file");
        assert!(tool_call.arguments.contains("path"));
    }

    #[test]
    fn test_parse_tool_calls_no_match() {
        let content = "This is just a regular response";
        let tool_call = parse_tool_calls(content);

        assert!(tool_call.is_none());
    }
}
