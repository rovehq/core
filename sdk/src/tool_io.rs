use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Input to a tool function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInput {
    pub method: String,
    pub params: HashMap<String, serde_json::Value>,
}

impl ToolInput {
    pub fn new(method: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            params: HashMap::new(),
        }
    }

    pub fn with_param(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.params.insert(key.into(), value);
        self
    }

    pub fn param_str(&self, key: &str) -> Result<String, ToolError> {
        self.params
            .get(key)
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| ToolError::MissingParameter(key.to_string()))
    }

    pub fn param_i64(&self, key: &str) -> Result<i64, ToolError> {
        self.params
            .get(key)
            .and_then(|v| v.as_i64())
            .ok_or_else(|| ToolError::MissingParameter(key.to_string()))
    }

    pub fn param_bool(&self, key: &str) -> Result<bool, ToolError> {
        self.params
            .get(key)
            .and_then(|v| v.as_bool())
            .ok_or_else(|| ToolError::MissingParameter(key.to_string()))
    }

    pub fn param_str_opt(&self, key: &str) -> Option<String> {
        self.params.get(key).and_then(|v| v.as_str()).map(String::from)
    }

    pub fn param_i64_opt(&self, key: &str) -> Option<i64> {
        self.params.get(key).and_then(|v| v.as_i64())
    }

    pub fn param_bool_opt(&self, key: &str) -> Option<bool> {
        self.params.get(key).and_then(|v| v.as_bool())
    }

    pub fn param_json(&self, key: &str) -> Result<&serde_json::Value, ToolError> {
        self.params
            .get(key)
            .ok_or_else(|| ToolError::MissingParameter(key.to_string()))
    }
}

/// Output from a tool function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub success: bool,
    pub data: serde_json::Value,
    pub error: Option<String>,
}

impl ToolOutput {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            success: true,
            data: serde_json::json!({ "text": text.into() }),
            error: None,
        }
    }

    pub fn json(data: serde_json::Value) -> Self {
        Self {
            success: true,
            data,
            error: None,
        }
    }

    pub fn error(error: impl Into<String>) -> Self {
        Self {
            success: false,
            data: serde_json::Value::Null,
            error: Some(error.into()),
        }
    }

    pub fn empty() -> Self {
        Self {
            success: true,
            data: serde_json::Value::Null,
            error: None,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Tool-specific errors.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("Missing parameter: {0}")]
    MissingParameter(String),

    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    #[error("Unknown method: {0}")]
    UnknownMethod(String),
}
