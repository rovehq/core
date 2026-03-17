//! Working Memory for Agent Loop
//!
//! Manages conversation history within context window limits. The working memory
//! stores messages in order (system prompt, user messages, assistant responses,
//! tool calls, tool results) and automatically trims old messages when approaching
//! the context limit while preserving the system prompt and recent messages.

use crate::llm::{Message, MessageRole};

/// Default context limit in tokens (conservative estimate for most models)
const DEFAULT_CONTEXT_LIMIT: usize = 8000;

/// Average tokens per character (rough estimate: 1 token ≈ 4 characters)
const CHARS_PER_TOKEN: usize = 4;

/// Working memory that manages conversation history within context limits
#[derive(Debug, Clone)]
pub struct WorkingMemory {
    /// All messages in the conversation
    messages: Vec<Message>,

    /// Maximum number of tokens allowed in context
    context_limit: usize,

    /// Current estimated token count
    token_count: usize,
}

impl WorkingMemory {
    /// Create a new working memory with default context limit
    pub fn new() -> Self {
        Self::with_limit(DEFAULT_CONTEXT_LIMIT)
    }

    /// Create a new working memory with a specific context limit
    pub fn with_limit(context_limit: usize) -> Self {
        Self {
            messages: Vec::new(),
            context_limit,
            token_count: 0,
        }
    }

    /// Add a message to the working memory
    ///
    /// If adding the message would exceed the context limit, old messages
    /// are trimmed (keeping the system prompt and recent messages).
    pub fn add_message(&mut self, message: Message) {
        let message_tokens = Self::estimate_tokens(&message);

        // Add the message
        self.messages.push(message);
        self.token_count += message_tokens;

        // Trim if necessary
        if self.token_count > self.context_limit {
            self.trim_messages();
        }
    }

    /// Get all messages in the conversation history
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// Get the current token count
    pub fn token_count(&self) -> usize {
        self.token_count
    }

    /// Get the context limit
    pub fn context_limit(&self) -> usize {
        self.context_limit
    }

    /// Clear all messages from working memory
    pub fn clear(&mut self) {
        self.messages.clear();
        self.token_count = 0;
    }

    /// Trim old messages to stay within context limit
    ///
    /// Strategy:
    /// 1. Always keep the system prompt (first message if it's a system message)
    /// 2. Remove oldest messages (after system prompt) until we're under the limit
    /// 3. Keep at least the most recent exchange (last user message + assistant response)
    fn trim_messages(&mut self) {
        // If we only have a few messages, don't trim
        if self.messages.len() <= 3 {
            return;
        }

        // Find the system prompt (if any)
        let has_system_prompt = self
            .messages
            .first()
            .map(|m| m.role == MessageRole::System)
            .unwrap_or(false);

        let system_prompt_count = if has_system_prompt { 1 } else { 0 };

        // Keep removing messages from the middle until we're under the limit
        // We want to keep: [system prompt] + [recent messages]
        while self.token_count > self.context_limit && self.messages.len() > system_prompt_count + 2
        {
            // Remove the oldest non-system message (index 1 if system prompt exists, 0 otherwise)
            let remove_index = system_prompt_count;

            if remove_index < self.messages.len() {
                let removed = self.messages.remove(remove_index);
                let removed_tokens = Self::estimate_tokens(&removed);
                self.token_count = self.token_count.saturating_sub(removed_tokens);
            } else {
                break;
            }
        }
    }

    /// Estimate the number of tokens in a message
    ///
    /// This is a rough estimate based on character count. Different tokenizers
    /// will produce different results, but this provides a reasonable approximation.
    fn estimate_tokens(message: &Message) -> usize {
        // Count characters in content
        let content_chars = message.content.len();

        // Count characters in tool_call_id if present
        let tool_call_chars = message
            .tool_call_id
            .as_ref()
            .map(|id| id.len())
            .unwrap_or(0);

        // Add overhead for role and structure (roughly 10 tokens)
        let overhead = 10;

        // Convert to tokens
        let total_chars = content_chars + tool_call_chars;
        let content_tokens = total_chars.div_ceil(CHARS_PER_TOKEN);

        content_tokens + overhead
    }
}

impl Default for WorkingMemory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
