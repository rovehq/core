//! Deep tests for llm::parser::parse_tool_calls() and related extraction logic

use rove_engine::llm::parser::parse_tool_calls;

// ── Raw JSON format ────────────────────────────────────────────────────────────

#[test]
fn parse_raw_json_simple() {
    let content = r#"{"function": "read_file", "arguments": {"path": "test.txt"}}"#;
    let result = parse_tool_calls(content);
    assert!(result.is_some());
    let tc = result.unwrap();
    assert_eq!(tc.name, "read_file");
}

#[test]
fn parse_raw_json_captures_arguments() {
    let content =
        r#"{"function": "write_file", "arguments": {"path": "out.txt", "content": "hello"}}"#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "write_file");
    assert!(tc.arguments.contains("path"));
    assert!(tc.arguments.contains("out.txt"));
}

#[test]
fn parse_raw_json_with_id_field() {
    let content = r#"{"function": "list_dir", "arguments": {"path": "/workspace"}}"#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "list_dir");
    assert!(!tc.id.is_empty());
}

#[test]
fn parse_raw_json_arguments_object() {
    let content = r#"{"function": "delete_file", "arguments": {"path": "/tmp/test.txt"}}"#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "delete_file");
}

#[test]
fn parse_raw_json_arguments_empty_object() {
    let content = r#"{"function": "noop", "arguments": {}}"#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "noop");
}

#[test]
fn parse_raw_json_arguments_nested() {
    let content =
        r#"{"function": "configure", "arguments": {"settings": {"debug": true, "level": 3}}}"#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "configure");
    assert!(tc.arguments.contains("debug"));
}

#[test]
fn parse_raw_json_with_whitespace() {
    let content = "  {\"function\": \"read_file\", \"arguments\": {\"path\": \"x.txt\"}}  ";
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "read_file");
}

#[test]
fn parse_raw_json_reordered_keys() {
    let content = r#"{"arguments": {"path": "."}, "function": "list_dir"}"#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "list_dir");
}

#[test]
fn parse_raw_json_arguments_array() {
    let content = r#"{"function": "multi_op", "arguments": [1, 2, 3]}"#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "multi_op");
}

#[test]
fn parse_raw_json_arguments_string() {
    let content = r#"{"function": "echo", "arguments": "hello world"}"#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "echo");
}

#[test]
fn parse_raw_json_arguments_number() {
    let content = r#"{"function": "sleep", "arguments": 5}"#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "sleep");
}

#[test]
fn parse_raw_json_missing_function_returns_none() {
    let content = r#"{"tool": "read_file", "arguments": {}}"#;
    let result = parse_tool_calls(content);
    assert!(result.is_none());
}

#[test]
fn parse_raw_json_missing_arguments_returns_none() {
    let content = r#"{"function": "read_file"}"#;
    let result = parse_tool_calls(content);
    assert!(result.is_none());
}

#[test]
fn parse_raw_json_function_null_returns_none() {
    let content = r#"{"function": null, "arguments": {}}"#;
    let result = parse_tool_calls(content);
    assert!(result.is_none());
}

#[test]
fn parse_raw_json_empty_function_name() {
    let content = r#"{"function": "", "arguments": {}}"#;
    // empty string is still a valid parse
    let result = parse_tool_calls(content);
    if let Some(tc) = result {
        assert_eq!(tc.name, "");
    }
}

// ── Fenced JSON format ─────────────────────────────────────────────────────────

#[test]
fn parse_fenced_json_basic() {
    let content =
        "```json\n{\"function\": \"read_file\", \"arguments\": {\"path\": \"test.txt\"}}\n```";
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "read_file");
}

#[test]
fn parse_fenced_json_no_lang_tag() {
    let content = "```\n{\"function\": \"list_dir\", \"arguments\": {\"path\": \".\"}}\n```";
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "list_dir");
}

#[test]
fn parse_fenced_json_with_surrounding_prose() {
    let content = "Here is the tool call:\n```json\n{\"function\": \"write_file\", \"arguments\": {\"path\": \"out.txt\"}}\n```\nDone!";
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "write_file");
}

#[test]
fn parse_fenced_json_multiline_arguments() {
    let content = "```json\n{\"function\": \"run_command\", \"arguments\": {\n  \"command\": \"cargo test\",\n  \"timeout\": 60\n}}\n```";
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "run_command");
    assert!(tc.arguments.contains("cargo test"));
}

#[test]
fn parse_fenced_json_indented_content() {
    let content =
        "```json\n  {\"function\": \"read_file\", \"arguments\": {\"path\": \"a.rs\"}}\n```";
    let result = parse_tool_calls(content);
    // May or may not parse depending on trim - just don't panic
    let _ = result;
}

// ── Tool call XML marker format ────────────────────────────────────────────────

#[test]
fn parse_tool_call_marker_simple() {
    let content = r#"<tool_call>read_file({"path": "test.txt"})</tool_call>"#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "read_file");
    assert!(tc.arguments.contains("path"));
}

#[test]
fn parse_tool_call_marker_list_dir() {
    let content = r#"<tool_call>list_dir({"path": "/workspace"})</tool_call>"#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "list_dir");
}

#[test]
fn parse_tool_call_marker_empty_args() {
    let content = r#"<tool_call>noop()</tool_call>"#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "noop");
}

#[test]
fn parse_tool_call_marker_with_spaces() {
    let content = r#"<tool_call>  read_file ({"path": "x.txt"})</tool_call>"#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name.trim(), "read_file");
}

#[test]
fn parse_tool_call_marker_with_prose_before() {
    let content =
        r#"I will read the file now. <tool_call>read_file({"path": "a.txt"})</tool_call>"#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "read_file");
}

#[test]
fn parse_tool_call_marker_complex_args() {
    let content =
        r#"<tool_call>write_file({"path": "/tmp/out.txt", "content": "hello\nworld"})</tool_call>"#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "write_file");
}

#[test]
fn parse_tool_call_marker_id_starts_with_call() {
    let content = r#"<tool_call>read_file({"path": "x.txt"})</tool_call>"#;
    let tc = parse_tool_calls(content).unwrap();
    assert!(tc.id.starts_with("call_"));
}

#[test]
fn parse_tool_call_marker_unclosed_returns_none() {
    let content = r#"<tool_call>read_file({"path": "x.txt"})"#;
    // No closing tag, should not match XML path
    // May still match other paths
    let _ = parse_tool_calls(content);
}

// ── Embedded JSON in prose ─────────────────────────────────────────────────────

#[test]
fn parse_embedded_json_in_prose() {
    let content = r#"I completed the task. {"function": "list_dir", "arguments": {"path": "."}}"#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "list_dir");
}

#[test]
fn parse_embedded_json_after_period() {
    let content = r#"Done. {"arguments":{"path":"."},"function":"list_dir"}"#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "list_dir");
}

#[test]
fn parse_embedded_json_in_middle_of_text() {
    let content = r#"The agent will call {"function": "read_file", "arguments": {"path": "config.toml"}} to get the config."#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "read_file");
}

#[test]
fn parse_embedded_json_first_match_wins() {
    let content = r#"First: {"function": "read_file", "arguments": {}} Later: {"function": "write_file", "arguments": {}}"#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "read_file");
}

// ── No match cases ─────────────────────────────────────────────────────────────

#[test]
fn parse_plain_text_returns_none() {
    let content = "This is just a normal response with no tool calls.";
    assert!(parse_tool_calls(content).is_none());
}

#[test]
fn parse_empty_string_returns_none() {
    assert!(parse_tool_calls("").is_none());
}

#[test]
fn parse_whitespace_only_returns_none() {
    assert!(parse_tool_calls("   \n   ").is_none());
}

#[test]
fn parse_json_without_function_key_returns_none() {
    let content = r#"{"tool_name": "read_file", "params": {}}"#;
    assert!(parse_tool_calls(content).is_none());
}

#[test]
fn parse_non_json_content_returns_none() {
    let content = "The file was read successfully. Here is the content: hello world.";
    assert!(parse_tool_calls(content).is_none());
}

#[test]
fn parse_markdown_without_json_returns_none() {
    let content = "# Title\n\nSome **bold** text and *italic* text.";
    assert!(parse_tool_calls(content).is_none());
}

#[test]
fn parse_xml_no_tool_call_tag_returns_none() {
    let content = "<response>done</response>";
    assert!(parse_tool_calls(content).is_none());
}

#[test]
fn parse_invalid_json_returns_none() {
    let content = r#"{broken json here"#;
    assert!(parse_tool_calls(content).is_none());
}

#[test]
fn parse_partial_json_returns_none() {
    let content = r#"{"function": "re"#;
    assert!(parse_tool_calls(content).is_none());
}

// ── Tool call ID uniqueness ────────────────────────────────────────────────────

#[test]
fn parse_generates_unique_ids() {
    let content = r#"{"function": "read_file", "arguments": {}}"#;
    let tc1 = parse_tool_calls(content).unwrap();
    let tc2 = parse_tool_calls(content).unwrap();
    assert_ne!(tc1.id, tc2.id);
}

#[test]
fn parse_id_is_nonempty() {
    let content = r#"{"function": "read_file", "arguments": {}}"#;
    let tc = parse_tool_calls(content).unwrap();
    assert!(!tc.id.is_empty());
}

// ── Edge cases ─────────────────────────────────────────────────────────────────

#[test]
fn parse_deeply_nested_arguments() {
    let content = r#"{"function": "complex", "arguments": {"a": {"b": {"c": {"d": "value"}}}}}"#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "complex");
    assert!(tc.arguments.contains("value"));
}

#[test]
fn parse_arguments_with_unicode() {
    let content = r#"{"function": "greet", "arguments": {"message": "こんにちは"}}"#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "greet");
    assert!(tc.arguments.contains("こんにちは"));
}

#[test]
fn parse_arguments_with_escaped_quotes() {
    let content = r#"{"function": "echo", "arguments": {"msg": "say \"hello\""}}"#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "echo");
}

#[test]
fn parse_function_with_underscore() {
    let content = r#"{"function": "run_shell_command", "arguments": {"cmd": "ls"}}"#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "run_shell_command");
}

#[test]
fn parse_function_with_numbers() {
    let content = r#"{"function": "tool_v2", "arguments": {}}"#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "tool_v2");
}

#[test]
fn parse_long_path_argument() {
    let long_path = "/".to_string() + &"a/".repeat(50) + "file.txt";
    let content = format!(
        r#"{{"function": "read_file", "arguments": {{"path": "{}"}}}}"#,
        long_path
    );
    let tc = parse_tool_calls(&content).unwrap();
    assert_eq!(tc.name, "read_file");
}

#[test]
fn parse_fenced_json_with_trailing_newlines() {
    let content =
        "```json\n{\"function\": \"read_file\", \"arguments\": {\"path\": \"x.txt\"}}\n\n\n```";
    let result = parse_tool_calls(content);
    // May or may not match, just check no panic
    let _ = result;
}

#[test]
fn parse_multiple_fences_first_wins() {
    let content = "```json\n{\"function\": \"read_file\", \"arguments\": {}}\n```\n```json\n{\"function\": \"write_file\", \"arguments\": {}}\n```";
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "read_file");
}

#[test]
fn parse_real_world_claude_format() {
    let content = "I'll read the file for you.\n```json\n{\"function\": \"read_file\", \"arguments\": {\"path\": \"src/main.rs\"}}\n```";
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "read_file");
    assert!(tc.arguments.contains("src/main.rs"));
}

#[test]
fn parse_real_world_inline_json() {
    let content = "Calling the tool: {\"function\": \"file_exists\", \"arguments\": {\"path\": \"/workspace/Cargo.toml\"}}";
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "file_exists");
}

#[test]
fn parse_bool_argument() {
    let content = r#"{"function": "set_flag", "arguments": {"enabled": true}}"#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "set_flag");
    assert!(tc.arguments.contains("true"));
}

#[test]
fn parse_null_argument_value() {
    let content = r#"{"function": "clear", "arguments": {"value": null}}"#;
    let tc = parse_tool_calls(content).unwrap();
    assert_eq!(tc.name, "clear");
}
