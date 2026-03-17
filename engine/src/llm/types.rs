use serde::{Deserialize, Serialize};
use std::fmt;

pub use sdk::ToolSchema;

/// Message in a conversation history.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    /// Role of the message sender (user, assistant, system, tool).
    pub role: MessageRole,
    /// Content of the message.
    pub content: String,
    /// Optional tool call ID for tool result messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
            tool_call_id: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
            tool_call_id: None,
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: content.into(),
            tool_call_id: None,
        }
    }

    pub fn tool_result(content: impl Into<String>, tool_call_id: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Tool,
            content: content.into(),
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

/// Role of a message sender.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

impl fmt::Display for MessageRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MessageRole::User => write!(f, "user"),
            MessageRole::Assistant => write!(f, "assistant"),
            MessageRole::System => write!(f, "system"),
            MessageRole::Tool => write!(f, "tool"),
        }
    }
}

/// Response from an LLM provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LLMResponse {
    ToolCall(ToolCall),
    FinalAnswer(FinalAnswer),
}

/// Tool call request from the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Unique identifier for this tool call.
    pub id: String,
    /// Name of the tool to call.
    pub name: String,
    /// Arguments to pass to the tool (JSON string).
    pub arguments: String,
}

impl ToolCall {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            arguments: arguments.into(),
        }
    }
}

/// Final answer from the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalAnswer {
    pub content: String,
}

impl FinalAnswer {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_creation() {
        let user_msg = Message::user("Hello");
        assert_eq!(user_msg.role, MessageRole::User);
        assert_eq!(user_msg.content, "Hello");
        assert_eq!(user_msg.tool_call_id, None);

        let assistant_msg = Message::assistant("Hi there");
        assert_eq!(assistant_msg.role, MessageRole::Assistant);
        assert_eq!(assistant_msg.content, "Hi there");

        let system_msg = Message::system("You are a helpful assistant");
        assert_eq!(system_msg.role, MessageRole::System);

        let tool_msg = Message::tool_result("result", "call_123");
        assert_eq!(tool_msg.role, MessageRole::Tool);
        assert_eq!(tool_msg.tool_call_id, Some("call_123".to_string()));
    }

    #[test]
    fn test_tool_call_creation() {
        let tool_call = ToolCall::new("call_123", "read_file", r#"{"path": "test.txt"}"#);
        assert_eq!(tool_call.id, "call_123");
        assert_eq!(tool_call.name, "read_file");
        assert_eq!(tool_call.arguments, r#"{"path": "test.txt"}"#);
    }

    #[test]
    fn test_final_answer_creation() {
        let answer = FinalAnswer::new("The answer is 42");
        assert_eq!(answer.content, "The answer is 42");
    }

    #[test]
    fn test_message_serialization() {
        let msg = Message::user("test");
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, deserialized);
    }

    #[test]
    fn test_llm_response_serialization() {
        let tool_call = LLMResponse::ToolCall(ToolCall::new("id", "name", "{}"));
        let json = serde_json::to_string(&tool_call).unwrap();
        assert!(json.contains(r#""type":"tool_call"#));

        let final_answer = LLMResponse::FinalAnswer(FinalAnswer::new("answer"));
        let json = serde_json::to_string(&final_answer).unwrap();
        assert!(json.contains(r#""type":"final_answer"#));
    }
}
