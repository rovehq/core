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
mod tests {
    use super::*;

    #[test]
    fn test_new_working_memory() {
        let memory = WorkingMemory::new();
        assert_eq!(memory.messages().len(), 0);
        assert_eq!(memory.token_count(), 0);
        assert_eq!(memory.context_limit(), DEFAULT_CONTEXT_LIMIT);
    }

    #[test]
    fn test_with_limit() {
        let memory = WorkingMemory::with_limit(4000);
        assert_eq!(memory.context_limit(), 4000);
    }

    #[test]
    fn test_add_message() {
        let mut memory = WorkingMemory::new();

        memory.add_message(Message::system("You are a helpful assistant"));
        assert_eq!(memory.messages().len(), 1);
        assert!(memory.token_count() > 0);

        memory.add_message(Message::user("Hello"));
        assert_eq!(memory.messages().len(), 2);
    }

    #[test]
    fn test_clear() {
        let mut memory = WorkingMemory::new();
        memory.add_message(Message::user("Hello"));
        memory.add_message(Message::assistant("Hi"));

        assert_eq!(memory.messages().len(), 2);
        assert!(memory.token_count() > 0);

        memory.clear();
        assert_eq!(memory.messages().len(), 0);
        assert_eq!(memory.token_count(), 0);
    }

    #[test]
    fn test_estimate_tokens() {
        let short_msg = Message::user("Hi");
        let short_tokens = WorkingMemory::estimate_tokens(&short_msg);
        assert!(short_tokens > 0);
        assert!(short_tokens < 20); // Should be small

        let long_msg =
            Message::user("This is a much longer message with many more words and characters");
        let long_tokens = WorkingMemory::estimate_tokens(&long_msg);
        assert!(long_tokens > short_tokens);
    }

    #[test]
    fn test_token_estimation_with_tool_call_id() {
        let msg_without_id = Message::user("test");
        let tokens_without = WorkingMemory::estimate_tokens(&msg_without_id);

        let msg_with_id = Message::tool_result("test", "call_123456789");
        let tokens_with = WorkingMemory::estimate_tokens(&msg_with_id);

        assert!(tokens_with > tokens_without);
    }

    #[test]
    fn test_trimming_preserves_system_prompt() {
        let mut memory = WorkingMemory::with_limit(100); // Very small limit

        // Add system prompt
        memory.add_message(Message::system("You are a helpful assistant"));

        // Add many messages to trigger trimming
        for i in 0..20 {
            memory.add_message(Message::user(format!("Message {}", i)));
            memory.add_message(Message::assistant(format!("Response {}", i)));
        }

        // System prompt should still be first
        assert_eq!(memory.messages().first().unwrap().role, MessageRole::System);
        assert_eq!(
            memory.messages().first().unwrap().content,
            "You are a helpful assistant"
        );

        // Should have trimmed to stay under limit
        assert!(memory.token_count() <= memory.context_limit());
    }

    #[test]
    fn test_trimming_keeps_recent_messages() {
        let mut memory = WorkingMemory::with_limit(100); // Very small limit

        // Add system prompt
        memory.add_message(Message::system("System"));

        // Add many messages
        for i in 0..10 {
            memory.add_message(Message::user(format!("User {}", i)));
            memory.add_message(Message::assistant(format!("Assistant {}", i)));
        }

        // The most recent messages should be preserved
        let messages = memory.messages();
        let last_user = messages
            .iter()
            .rev()
            .find(|m| m.role == MessageRole::User)
            .unwrap();

        assert!(last_user.content.contains("User 9"));
    }

    #[test]
    fn test_no_trimming_with_few_messages() {
        let mut memory = WorkingMemory::with_limit(50); // Small limit

        memory.add_message(Message::system("System"));
        memory.add_message(Message::user("Hello"));
        memory.add_message(Message::assistant("Hi"));

        // Should not trim with only 3 messages
        assert_eq!(memory.messages().len(), 3);
    }

    #[test]
    fn test_trimming_without_system_prompt() {
        let mut memory = WorkingMemory::with_limit(100); // Small limit

        // Add many messages without system prompt
        for i in 0..15 {
            memory.add_message(Message::user(format!("Message {}", i)));
            memory.add_message(Message::assistant(format!("Response {}", i)));
        }

        // Should have trimmed
        assert!(memory.messages().len() < 30);
        assert!(memory.token_count() <= memory.context_limit());

        // Most recent messages should be preserved
        let last_msg = memory.messages().last().unwrap();
        assert!(last_msg.content.contains("14"));
    }

    #[test]
    fn test_messages_getter() {
        let mut memory = WorkingMemory::new();
        memory.add_message(Message::user("Test"));

        let messages = memory.messages();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "Test");
    }

    #[test]
    fn test_context_limit_enforcement() {
        let mut memory = WorkingMemory::with_limit(200);

        // Add messages until we exceed the limit
        for i in 0..50 {
            memory.add_message(Message::user(format!("This is message number {}", i)));
        }

        // Token count should be at or below the limit
        assert!(memory.token_count() <= memory.context_limit());
    }

    #[test]
    fn test_default_implementation() {
        let memory = WorkingMemory::default();
        assert_eq!(memory.context_limit(), DEFAULT_CONTEXT_LIMIT);
        assert_eq!(memory.messages().len(), 0);
    }
}
