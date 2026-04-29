//! Extended tests for llm::types — Message, MessageRole, ToolCall, FinalAnswer, LLMResponse

use rove_engine::llm::types::{FinalAnswer, LLMResponse, Message, MessageRole, ToolCall};

// ── MessageRole display ───────────────────────────────────────────────────────

#[test]
fn user_role_display() {
    assert_eq!(format!("{}", MessageRole::User), "user");
}

#[test]
fn assistant_role_display() {
    assert_eq!(format!("{}", MessageRole::Assistant), "assistant");
}

#[test]
fn system_role_display() {
    assert_eq!(format!("{}", MessageRole::System), "system");
}

#[test]
fn tool_role_display() {
    assert_eq!(format!("{}", MessageRole::Tool), "tool");
}

// ── MessageRole equality ──────────────────────────────────────────────────────

#[test]
fn user_eq_user() {
    assert_eq!(MessageRole::User, MessageRole::User);
}

#[test]
fn assistant_ne_user() {
    assert_ne!(MessageRole::Assistant, MessageRole::User);
}

#[test]
fn system_ne_assistant() {
    assert_ne!(MessageRole::System, MessageRole::Assistant);
}

#[test]
fn tool_ne_system() {
    assert_ne!(MessageRole::Tool, MessageRole::System);
}

// ── MessageRole clone ─────────────────────────────────────────────────────────

#[test]
fn role_clone_user() {
    let r = MessageRole::User;
    let r2 = r;
    assert_eq!(r, r2);
}

#[test]
fn role_clone_assistant() {
    let r = MessageRole::Assistant;
    assert_eq!(r, r.clone());
}

// ── MessageRole serialization ─────────────────────────────────────────────────

#[test]
fn user_serializes_to_user() {
    let j = serde_json::to_string(&MessageRole::User).unwrap();
    assert!(j.contains("user"));
}

#[test]
fn assistant_serializes_to_assistant() {
    let j = serde_json::to_string(&MessageRole::Assistant).unwrap();
    assert!(j.contains("assistant"));
}

#[test]
fn system_serializes_to_system() {
    let j = serde_json::to_string(&MessageRole::System).unwrap();
    assert!(j.contains("system"));
}

#[test]
fn tool_role_serializes() {
    let j = serde_json::to_string(&MessageRole::Tool).unwrap();
    assert!(j.contains("tool"));
}

// ── Message construction ──────────────────────────────────────────────────────

#[test]
fn message_user_role() {
    let m = Message::user("hello");
    assert_eq!(m.role, MessageRole::User);
}

#[test]
fn message_user_content() {
    let m = Message::user("hello world");
    assert_eq!(m.content, "hello world");
}

#[test]
fn message_assistant_role() {
    let m = Message::assistant("I will help");
    assert_eq!(m.role, MessageRole::Assistant);
}

#[test]
fn message_assistant_content() {
    let m = Message::assistant("sure");
    assert_eq!(m.content, "sure");
}

#[test]
fn message_system_role() {
    let m = Message::system("you are an AI");
    assert_eq!(m.role, MessageRole::System);
}

#[test]
fn message_system_content() {
    let m = Message::system("instructions here");
    assert_eq!(m.content, "instructions here");
}

#[test]
fn message_tool_result_role() {
    let m = Message::tool_result("result", "call-123");
    assert_eq!(m.role, MessageRole::Tool);
}

#[test]
fn message_tool_result_content() {
    let m = Message::tool_result("output data", "call-456");
    assert_eq!(m.content, "output data");
}

#[test]
fn message_tool_result_has_call_id() {
    let m = Message::tool_result("output", "call-789");
    assert_eq!(m.tool_call_id, Some("call-789".to_string()));
}

#[test]
fn message_user_no_tool_call_id() {
    let m = Message::user("hi");
    assert!(m.tool_call_id.is_none());
}

#[test]
fn message_empty_content() {
    let m = Message::user("");
    assert_eq!(m.content, "");
}

#[test]
fn message_long_content() {
    let content = "x".repeat(10_000);
    let m = Message::user(content.clone());
    assert_eq!(m.content, content);
}

#[test]
fn message_clone() {
    let m = Message::user("hello");
    let m2 = m.clone();
    assert_eq!(m.content, m2.content);
    assert_eq!(m.role, m2.role);
}

#[test]
fn message_equality() {
    let m1 = Message::user("hello");
    let m2 = Message::user("hello");
    assert_eq!(m1, m2);
}

#[test]
fn message_inequality_different_role() {
    let m1 = Message::user("hello");
    let m2 = Message::assistant("hello");
    assert_ne!(m1, m2);
}

#[test]
fn message_inequality_different_content() {
    let m1 = Message::user("hello");
    let m2 = Message::user("world");
    assert_ne!(m1, m2);
}

#[test]
fn message_serializes() {
    let m = Message::user("hi there");
    let j = serde_json::to_string(&m).unwrap();
    assert!(j.contains("hi there"));
    assert!(j.contains("user"));
}

#[test]
fn message_round_trips() {
    let m = Message::user("test content");
    let j = serde_json::to_string(&m).unwrap();
    let m2: Message = serde_json::from_str(&j).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn message_unicode_content() {
    let m = Message::user("こんにちは世界 🌍");
    assert_eq!(m.content, "こんにちは世界 🌍");
}

// ── ToolCall construction ─────────────────────────────────────────────────────

#[test]
fn tool_call_new_has_name() {
    let tc = ToolCall::new("call-1", "read_file", r#"{"path": "/tmp"}"#);
    assert_eq!(tc.name, "read_file");
}

#[test]
fn tool_call_new_has_arguments() {
    let tc = ToolCall::new("call-1", "list_dir", r#"{"path": "/workspace"}"#);
    assert!(tc.arguments.contains("workspace"));
}

#[test]
fn tool_call_has_id() {
    let tc = ToolCall::new("my-id", "read_file", "{}");
    assert_eq!(tc.id, "my-id");
}

#[test]
fn tool_call_clone() {
    let tc = ToolCall::new("id-1", "exec", r#"{"cmd": "ls"}"#);
    let tc2 = tc.clone();
    assert_eq!(tc.name, tc2.name);
    assert_eq!(tc.id, tc2.id);
}

#[test]
fn tool_call_serializes() {
    let tc = ToolCall::new("id-1", "write_file", r#"{"path": "a.txt"}"#);
    let j = serde_json::to_string(&tc).unwrap();
    assert!(j.contains("write_file"));
    assert!(j.contains("a.txt"));
}

#[test]
fn tool_call_debug() {
    let tc = ToolCall::new("id-1", "list_dir", "{}");
    let s = format!("{:?}", tc);
    assert!(s.contains("list_dir"));
}

#[test]
fn tool_call_round_trip() {
    let tc = ToolCall::new("id-1", "read_file", r#"{"path": "/foo"}"#);
    let j = serde_json::to_string(&tc).unwrap();
    let tc2: ToolCall = serde_json::from_str(&j).unwrap();
    assert_eq!(tc.name, tc2.name);
    assert_eq!(tc.id, tc2.id);
}

// ── FinalAnswer construction ──────────────────────────────────────────────────

#[test]
fn final_answer_new_has_content() {
    let fa = FinalAnswer::new("The answer is 42");
    assert_eq!(fa.content, "The answer is 42");
}

#[test]
fn final_answer_empty_content() {
    let fa = FinalAnswer::new("");
    assert_eq!(fa.content, "");
}

#[test]
fn final_answer_clone() {
    let fa = FinalAnswer::new("done");
    let fa2 = fa.clone();
    assert_eq!(fa.content, fa2.content);
}

#[test]
fn final_answer_serializes() {
    let fa = FinalAnswer::new("completed task");
    let j = serde_json::to_string(&fa).unwrap();
    assert!(j.contains("completed task"));
}

#[test]
fn final_answer_round_trip() {
    let fa = FinalAnswer::new("The task is complete");
    let j = serde_json::to_string(&fa).unwrap();
    let fa2: FinalAnswer = serde_json::from_str(&j).unwrap();
    assert_eq!(fa.content, fa2.content);
}

// ── LLMResponse variants ──────────────────────────────────────────────────────

#[test]
fn llm_response_tool_call_variant() {
    let tc = ToolCall::new("id-1", "read_file", r#"{"path": "/tmp"}"#);
    let resp = LLMResponse::ToolCall(tc);
    assert!(matches!(resp, LLMResponse::ToolCall(_)));
}

#[test]
fn llm_response_final_answer_variant() {
    let fa = FinalAnswer::new("done");
    let resp = LLMResponse::FinalAnswer(fa);
    assert!(matches!(resp, LLMResponse::FinalAnswer(_)));
}

#[test]
fn llm_response_tool_call_serializes() {
    let tc = ToolCall::new("id-1", "read_file", r#"{"path": "/tmp"}"#);
    let resp = LLMResponse::ToolCall(tc);
    let j = serde_json::to_string(&resp).unwrap();
    assert!(j.contains("read_file"));
}

#[test]
fn llm_response_final_answer_serializes() {
    let fa = FinalAnswer::new("done");
    let resp = LLMResponse::FinalAnswer(fa);
    let j = serde_json::to_string(&resp).unwrap();
    assert!(j.contains("done"));
}

#[test]
fn llm_response_clone_tool_call() {
    let tc = ToolCall::new("id-1", "read_file", "{}");
    let resp = LLMResponse::ToolCall(tc);
    let resp2 = resp.clone();
    let j1 = serde_json::to_string(&resp).unwrap();
    let j2 = serde_json::to_string(&resp2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn llm_response_clone_final_answer() {
    let fa = FinalAnswer::new("done");
    let resp = LLMResponse::FinalAnswer(fa);
    let resp2 = resp.clone();
    let j1 = serde_json::to_string(&resp).unwrap();
    let j2 = serde_json::to_string(&resp2).unwrap();
    assert_eq!(j1, j2);
}

// ── Message collections ───────────────────────────────────────────────────────

#[test]
fn messages_in_vec_accessible_by_index() {
    let msgs = [
        Message::system("sys"),
        Message::user("hi"),
        Message::assistant("hello"),
    ];
    assert_eq!(msgs[0].role, MessageRole::System);
    assert_eq!(msgs[1].role, MessageRole::User);
    assert_eq!(msgs[2].role, MessageRole::Assistant);
}

#[test]
fn message_vec_len() {
    let msgs = [Message::user("a"), Message::user("b"), Message::user("c")];
    assert_eq!(msgs.len(), 3);
}

#[test]
fn message_filter_by_role() {
    let msgs = [
        Message::system("sys"),
        Message::user("hi"),
        Message::assistant("hello"),
        Message::user("bye"),
    ];
    let user_msgs: Vec<_> = msgs
        .iter()
        .filter(|m| m.role == MessageRole::User)
        .collect();
    assert_eq!(user_msgs.len(), 2);
}
