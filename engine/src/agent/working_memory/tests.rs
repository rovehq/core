use super::WorkingMemory;
use crate::llm::{Message, MessageRole};

#[test]
fn test_new_working_memory() {
    let memory = WorkingMemory::new();
    assert_eq!(memory.messages().len(), 0);
    assert_eq!(memory.token_count(), 0);
    assert_eq!(memory.context_limit(), 8000);
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
    assert!(short_tokens < 20);

    let long_msg = Message::user("This is a much longer message with many more words and characters");
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
    let mut memory = WorkingMemory::with_limit(100);
    memory.add_message(Message::system("You are a helpful assistant"));

    for i in 0..20 {
        memory.add_message(Message::user(format!("Message {}", i)));
        memory.add_message(Message::assistant(format!("Response {}", i)));
    }

    assert_eq!(memory.messages().first().unwrap().role, MessageRole::System);
    assert_eq!(
        memory.messages().first().unwrap().content,
        "You are a helpful assistant"
    );
    assert!(memory.token_count() <= memory.context_limit());
}

#[test]
fn test_trimming_keeps_recent_messages() {
    let mut memory = WorkingMemory::with_limit(100);
    memory.add_message(Message::system("System"));

    for i in 0..10 {
        memory.add_message(Message::user(format!("User {}", i)));
        memory.add_message(Message::assistant(format!("Assistant {}", i)));
    }

    let messages = memory.messages();
    let last_user = messages
        .iter()
        .rev()
        .find(|message| message.role == MessageRole::User)
        .unwrap();

    assert!(last_user.content.contains("User 9"));
}

#[test]
fn test_no_trimming_with_few_messages() {
    let mut memory = WorkingMemory::with_limit(50);
    memory.add_message(Message::system("System"));
    memory.add_message(Message::user("Hello"));
    memory.add_message(Message::assistant("Hi"));

    assert_eq!(memory.messages().len(), 3);
}

#[test]
fn test_trimming_without_system_prompt() {
    let mut memory = WorkingMemory::with_limit(100);

    for i in 0..15 {
        memory.add_message(Message::user(format!("Message {}", i)));
        memory.add_message(Message::assistant(format!("Response {}", i)));
    }

    assert!(memory.messages().len() < 30);
    assert!(memory.token_count() <= memory.context_limit());

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

    for i in 0..50 {
        memory.add_message(Message::user(format!("This is message number {}", i)));
    }

    assert!(memory.token_count() <= memory.context_limit());
}

#[test]
fn test_default_implementation() {
    let memory = WorkingMemory::default();
    assert_eq!(memory.context_limit(), 8000);
    assert_eq!(memory.messages().len(), 0);
}
