//! Memory system utility functions
//!
//! Contains helper functions for string processing and session memory management.

use crate::agent::WorkingMemory;
use crate::conductor::types::MemoryBudget;
use crate::llm::Message;

/// Truncate a string to `max_len` chars, appending "…" if truncated.
pub fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len])
    }
}

/// Strip markdown code fences from LLM output.
/// Handles ```json ... ``` and ``` ... ```.
pub fn strip_markdown_fences(text: &str) -> String {
    let trimmed = text.trim();

    // Check for opening fence
    if let Some(rest) = trimmed.strip_prefix("```") {
        // Skip language tag on the same line
        let body = if let Some(newline_pos) = rest.find('\n') {
            &rest[newline_pos + 1..]
        } else {
            rest
        };

        // Strip closing fence
        if let Some(stripped) = body.strip_suffix("```") {
            return stripped.trim().to_string();
        }
        return body.trim().to_string();
    }

    trimmed.to_string()
}

/// SessionMemory wraps WorkingMemory with a token budget.
/// Used by ContextAssembler for session history management.
#[derive(Debug, Clone)]
pub struct SessionMemory {
    working_memory: WorkingMemory,
    _max_tokens: usize,
}

impl SessionMemory {
    /// Create a new session memory managed by the given budget
    pub fn new(budget: &MemoryBudget) -> Self {
        Self {
            working_memory: WorkingMemory::with_limit(budget.session_tokens),
            _max_tokens: budget.session_tokens,
        }
    }

    /// Add a message to the session
    pub fn add(&mut self, message: Message) {
        self.working_memory.add_message(message);
    }

    /// Add a user message
    pub fn add_user(&mut self, content: &str) {
        self.add(Message::user(content));
    }

    /// Add an assistant message
    pub fn add_assistant(&mut self, content: &str) {
        self.add(Message::assistant(content));
    }

    /// Retrieve all session messages
    pub fn messages(&self) -> &[Message] {
        self.working_memory.messages()
    }

    /// Get token count
    pub fn token_count(&self) -> usize {
        self.working_memory.token_count()
    }

    /// Clear all messages
    pub fn clear(&mut self) {
        self.working_memory.clear()
    }
}
