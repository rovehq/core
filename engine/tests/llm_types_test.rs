//! Tests for llm::types — Message, MessageRole, ToolCall, FinalAnswer, LLMResponse

use rove_engine::llm::{FinalAnswer, LLMResponse, Message, MessageRole, ToolCall};

// ── MessageRole tests ─────────────────────────────────────────────────────────

#[test]
fn message_role_user_display() {
    assert_eq!(format!("{}", MessageRole::User), "user");
}

#[test]
fn message_role_assistant_display() {
    assert_eq!(format!("{}", MessageRole::Assistant), "assistant");
}

#[test]
fn message_role_system_display() {
    assert_eq!(format!("{}", MessageRole::System), "system");
}

#[test]
fn message_role_tool_display() {
    assert_eq!(format!("{}", MessageRole::Tool), "tool");
}

#[test]
fn message_role_user_serializes_lowercase() {
    let j = serde_json::to_string(&MessageRole::User).unwrap();
    assert_eq!(j, r#""user""#);
}

#[test]
fn message_role_assistant_serializes_lowercase() {
    let j = serde_json::to_string(&MessageRole::Assistant).unwrap();
    assert_eq!(j, r#""assistant""#);
}

#[test]
fn message_role_system_serializes_lowercase() {
    let j = serde_json::to_string(&MessageRole::System).unwrap();
    assert_eq!(j, r#""system""#);
}

#[test]
fn message_role_tool_serializes_lowercase() {
    let j = serde_json::to_string(&MessageRole::Tool).unwrap();
    assert_eq!(j, r#""tool""#);
}

#[test]
fn message_role_user_deserializes() {
    let role: MessageRole = serde_json::from_str(r#""user""#).unwrap();
    assert_eq!(role, MessageRole::User);
}

#[test]
fn message_role_assistant_deserializes() {
    let role: MessageRole = serde_json::from_str(r#""assistant""#).unwrap();
    assert_eq!(role, MessageRole::Assistant);
}

#[test]
fn message_role_system_deserializes() {
    let role: MessageRole = serde_json::from_str(r#""system""#).unwrap();
    assert_eq!(role, MessageRole::System);
}

#[test]
fn message_role_tool_deserializes() {
    let role: MessageRole = serde_json::from_str(r#""tool""#).unwrap();
    assert_eq!(role, MessageRole::Tool);
}

#[test]
fn message_role_equality() {
    assert_eq!(MessageRole::User, MessageRole::User);
    assert_ne!(MessageRole::User, MessageRole::Assistant);
    assert_ne!(MessageRole::System, MessageRole::Tool);
}

#[test]
fn message_role_copy() {
    let role = MessageRole::User;
    let role2 = role;
    assert_eq!(role, role2);
}

// ── Message construction ──────────────────────────────────────────────────────

#[test]
fn message_user_has_user_role() {
    let msg = Message::user("hello");
    assert_eq!(msg.role, MessageRole::User);
}

#[test]
fn message_user_has_content() {
    let msg = Message::user("hello there");
    assert_eq!(msg.content, "hello there");
}

#[test]
fn message_user_no_tool_call_id() {
    let msg = Message::user("hello");
    assert!(msg.tool_call_id.is_none());
}

#[test]
fn message_assistant_has_assistant_role() {
    let msg = Message::assistant("I am an assistant");
    assert_eq!(msg.role, MessageRole::Assistant);
}

#[test]
fn message_assistant_has_content() {
    let msg = Message::assistant("response content");
    assert_eq!(msg.content, "response content");
}

#[test]
fn message_assistant_no_tool_call_id() {
    let msg = Message::assistant("hi");
    assert!(msg.tool_call_id.is_none());
}

#[test]
fn message_system_has_system_role() {
    let msg = Message::system("You are a helpful assistant.");
    assert_eq!(msg.role, MessageRole::System);
}

#[test]
fn message_system_has_content() {
    let msg = Message::system("system prompt here");
    assert_eq!(msg.content, "system prompt here");
}

#[test]
fn message_system_no_tool_call_id() {
    let msg = Message::system("prompt");
    assert!(msg.tool_call_id.is_none());
}

#[test]
fn message_tool_result_has_tool_role() {
    let msg = Message::tool_result("result", "call_123");
    assert_eq!(msg.role, MessageRole::Tool);
}

#[test]
fn message_tool_result_has_content() {
    let msg = Message::tool_result("the file contents", "call_abc");
    assert_eq!(msg.content, "the file contents");
}

#[test]
fn message_tool_result_has_tool_call_id() {
    let msg = Message::tool_result("result", "call_xyz");
    assert_eq!(msg.tool_call_id, Some("call_xyz".to_string()));
}

#[test]
fn message_user_from_string() {
    let msg = Message::user("test".to_string());
    assert_eq!(msg.content, "test");
}

#[test]
fn message_from_empty_content() {
    let msg = Message::user("");
    assert_eq!(msg.content, "");
}

// ── Message serialization ─────────────────────────────────────────────────────

#[test]
fn message_user_serializes_role() {
    let msg = Message::user("hi");
    let j = serde_json::to_value(&msg).unwrap();
    assert_eq!(j["role"], "user");
}

#[test]
fn message_tool_call_id_in_serialization() {
    let msg = Message::tool_result("result", "call_1");
    let j = serde_json::to_value(&msg).unwrap();
    assert_eq!(j["tool_call_id"], "call_1");
}

#[test]
fn message_no_tool_call_id_omitted_from_serialization() {
    let msg = Message::user("hi");
    let j = serde_json::to_value(&msg).unwrap();
    // tool_call_id should be absent or null
    assert!(j.get("tool_call_id").is_none() || j["tool_call_id"].is_null());
}

#[test]
fn message_roundtrip_user() {
    let msg = Message::user("roundtrip test");
    let json = serde_json::to_string(&msg).unwrap();
    let back: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(back, msg);
}

#[test]
fn message_roundtrip_assistant() {
    let msg = Message::assistant("assistant response");
    let json = serde_json::to_string(&msg).unwrap();
    let back: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(back, msg);
}

#[test]
fn message_roundtrip_system() {
    let msg = Message::system("system prompt");
    let json = serde_json::to_string(&msg).unwrap();
    let back: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(back, msg);
}

#[test]
fn message_roundtrip_tool_result() {
    let msg = Message::tool_result("result data", "call_99");
    let json = serde_json::to_string(&msg).unwrap();
    let back: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(back, msg);
}

#[test]
fn message_clone_is_equal() {
    let msg = Message::user("test");
    let cloned = msg.clone();
    assert_eq!(msg, cloned);
}

// ── ToolCall tests ─────────────────────────────────────────────────────────────

#[test]
fn tool_call_new_sets_id() {
    let tc = ToolCall::new("call_1", "read_file", r#"{"path":"x.txt"}"#);
    assert_eq!(tc.id, "call_1");
}

#[test]
fn tool_call_new_sets_name() {
    let tc = ToolCall::new("call_1", "read_file", r#"{}"#);
    assert_eq!(tc.name, "read_file");
}

#[test]
fn tool_call_new_sets_arguments() {
    let tc = ToolCall::new("call_1", "read_file", r#"{"path":"x.txt"}"#);
    assert_eq!(tc.arguments, r#"{"path":"x.txt"}"#);
}

#[test]
fn tool_call_from_strings() {
    let tc = ToolCall::new("id".to_string(), "name".to_string(), "args".to_string());
    assert_eq!(tc.id, "id");
    assert_eq!(tc.name, "name");
    assert_eq!(tc.arguments, "args");
}

#[test]
fn tool_call_serializes() {
    let tc = ToolCall::new("call_abc", "write_file", r#"{"path":"out.txt"}"#);
    let j = serde_json::to_value(&tc).unwrap();
    assert_eq!(j["id"], "call_abc");
    assert_eq!(j["name"], "write_file");
}

#[test]
fn tool_call_deserializes() {
    let json = r#"{"id":"call_1","name":"read_file","arguments":"{}"}"#;
    let tc: ToolCall = serde_json::from_str(json).unwrap();
    assert_eq!(tc.id, "call_1");
    assert_eq!(tc.name, "read_file");
}

#[test]
fn tool_call_roundtrip() {
    let tc = ToolCall::new("call_x", "list_dir", r#"{"path":"/tmp"}"#);
    let json = serde_json::to_string(&tc).unwrap();
    let back: ToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, tc.id);
    assert_eq!(back.name, tc.name);
    assert_eq!(back.arguments, tc.arguments);
}

#[test]
fn tool_call_empty_arguments() {
    let tc = ToolCall::new("call_empty", "noop", "");
    assert_eq!(tc.arguments, "");
}

#[test]
fn tool_call_clone_is_independent() {
    let tc = ToolCall::new("call_1", "read_file", r#"{}"#);
    let mut cloned = tc.clone();
    cloned.name = "write_file".to_string();
    assert_eq!(tc.name, "read_file");
}

// ── FinalAnswer tests ─────────────────────────────────────────────────────────

#[test]
fn final_answer_new_sets_content() {
    let fa = FinalAnswer::new("The answer is 42");
    assert_eq!(fa.content, "The answer is 42");
}

#[test]
fn final_answer_empty_content() {
    let fa = FinalAnswer::new("");
    assert_eq!(fa.content, "");
}

#[test]
fn final_answer_multiline() {
    let content = "Line 1\nLine 2\nLine 3";
    let fa = FinalAnswer::new(content);
    assert_eq!(fa.content, content);
}

#[test]
fn final_answer_serializes() {
    let fa = FinalAnswer::new("result");
    let j = serde_json::to_value(&fa).unwrap();
    assert_eq!(j["content"], "result");
}

#[test]
fn final_answer_roundtrip() {
    let fa = FinalAnswer::new("the final answer");
    let json = serde_json::to_string(&fa).unwrap();
    let back: FinalAnswer = serde_json::from_str(&json).unwrap();
    assert_eq!(back.content, fa.content);
}

#[test]
fn final_answer_from_string() {
    let fa = FinalAnswer::new("test".to_string());
    assert_eq!(fa.content, "test");
}

// ── LLMResponse tests ─────────────────────────────────────────────────────────

#[test]
fn llm_response_tool_call_variant() {
    let tc = ToolCall::new("id", "name", "{}");
    let resp = LLMResponse::ToolCall(tc);
    assert!(matches!(resp, LLMResponse::ToolCall(_)));
}

#[test]
fn llm_response_final_answer_variant() {
    let fa = FinalAnswer::new("answer");
    let resp = LLMResponse::FinalAnswer(fa);
    assert!(matches!(resp, LLMResponse::FinalAnswer(_)));
}

#[test]
fn llm_response_tool_call_serializes_type_field() {
    let tc = ToolCall::new("id", "name", "{}");
    let resp = LLMResponse::ToolCall(tc);
    let j = serde_json::to_value(&resp).unwrap();
    assert_eq!(j["type"], "tool_call");
}

#[test]
fn llm_response_final_answer_serializes_type_field() {
    let fa = FinalAnswer::new("answer");
    let resp = LLMResponse::FinalAnswer(fa);
    let j = serde_json::to_value(&resp).unwrap();
    assert_eq!(j["type"], "final_answer");
}

#[test]
fn llm_response_tool_call_roundtrip() {
    let tc = ToolCall::new("call_1", "read_file", r#"{"path":"x"}"#);
    let resp = LLMResponse::ToolCall(tc);
    let json = serde_json::to_string(&resp).unwrap();
    let back: LLMResponse = serde_json::from_str(&json).unwrap();
    if let LLMResponse::ToolCall(tc_back) = back {
        assert_eq!(tc_back.name, "read_file");
    } else {
        panic!("Expected ToolCall");
    }
}

#[test]
fn llm_response_final_answer_roundtrip() {
    let fa = FinalAnswer::new("42");
    let resp = LLMResponse::FinalAnswer(fa);
    let json = serde_json::to_string(&resp).unwrap();
    let back: LLMResponse = serde_json::from_str(&json).unwrap();
    if let LLMResponse::FinalAnswer(fa_back) = back {
        assert_eq!(fa_back.content, "42");
    } else {
        panic!("Expected FinalAnswer");
    }
}

// ── Message collections ────────────────────────────────────────────────────────

#[test]
fn message_vec_construction() {
    let messages = [
        Message::system("You are helpful"),
        Message::user("What is 2+2?"),
        Message::assistant("4"),
    ];
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].role, MessageRole::System);
    assert_eq!(messages[1].role, MessageRole::User);
    assert_eq!(messages[2].role, MessageRole::Assistant);
}

#[test]
fn message_vec_with_tool_result() {
    let messages = [
        Message::user("read the file"),
        Message::tool_result("file contents here", "call_1"),
    ];
    assert_eq!(messages[1].role, MessageRole::Tool);
    assert_eq!(messages[1].tool_call_id.as_deref(), Some("call_1"));
}

#[test]
fn messages_serialize_to_array() {
    let messages = vec![Message::user("hello"), Message::assistant("world")];
    let json = serde_json::to_string(&messages).unwrap();
    assert!(json.starts_with('['));
    assert!(json.contains("\"user\""));
    assert!(json.contains("\"assistant\""));
}

#[test]
fn message_with_special_characters() {
    let msg = Message::user("Test with special chars: <>&\"'\\n");
    assert!(msg.content.contains('<'));
}

#[test]
fn message_with_unicode() {
    let msg = Message::user("こんにちは世界");
    assert_eq!(msg.content, "こんにちは世界");
}

#[test]
fn message_with_long_content() {
    let long = "x".repeat(10000);
    let msg = Message::user(long.clone());
    assert_eq!(msg.content.len(), 10000);
}
