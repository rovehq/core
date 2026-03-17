use super::approvals::{park_approval, resolve_approval};
use super::types::{InlineKeyboardButton, InlineKeyboardMarkup, TelegramRateLimits};
use super::TelegramBot;
use crate::secrets::SecretManager;

#[test]
fn test_telegram_bot_creation() {
    let bot = TelegramBot::new("test_token".to_string(), vec![12345]);
    assert_eq!(bot.token, "test_token");
    assert_eq!(bot.allowed_users, vec![12345]);
    assert!(bot.agent.is_none());
    assert!(bot.confirmation_chat_id.is_none());
}

#[test]
fn test_telegram_bot_with_confirmation_chat() {
    let bot = TelegramBot::new("token".to_string(), vec![]).with_confirmation_chat(99999);
    assert_eq!(bot.confirmation_chat_id, Some(99999));
}

#[test]
fn test_rate_limits_general() {
    let mut limits = TelegramRateLimits::new();
    for _ in 0..60 {
        assert!(limits.check_general());
    }
    assert!(!limits.check_general());
}

#[test]
fn test_rate_limits_tier2() {
    let mut limits = TelegramRateLimits::new();
    for _ in 0..10 {
        assert!(limits.check_tier2());
    }
    assert!(!limits.check_tier2());
}

#[test]
fn test_secret_scrubbing_in_messages() {
    let manager = SecretManager::new("test");
    let text = "Error with key sk-1234567890abcdefghijklmnopqrstuvwxyz";
    let scrubbed = manager.scrub(text);
    assert!(!scrubbed.contains("sk-"));
    assert!(scrubbed.contains("[REDACTED]"));
}

#[test]
fn test_inline_keyboard_serialization() {
    let keyboard = InlineKeyboardMarkup {
        inline_keyboard: vec![vec![
            InlineKeyboardButton {
                text: "Approve".to_string(),
                callback_data: "approve:test".to_string(),
            },
            InlineKeyboardButton {
                text: "Deny".to_string(),
                callback_data: "deny:test".to_string(),
            },
        ]],
    };
    let json = serde_json::to_string(&keyboard).unwrap();
    assert!(json.contains("Approve"));
    assert!(json.contains("approve:test"));
}

#[test]
fn test_unauthorized_user_detection() {
    let bot = TelegramBot::new("token".to_string(), vec![111, 222]);
    assert!(!bot.allowed_users.contains(&333));
    assert!(bot.allowed_users.contains(&111));
}

#[test]
fn test_empty_allowed_users_allows_all() {
    let bot = TelegramBot::new("token".to_string(), vec![]);
    assert!(bot.allowed_users.is_empty());
}

#[tokio::test]
async fn test_pending_approval_resolve() {
    let op_key = "approve_test:run_command:unique123".to_string();
    let rx = park_approval(op_key.clone()).await;
    resolve_approval(&op_key, true).await;
    let approved = rx.await.unwrap();
    assert!(approved);
}

#[tokio::test]
async fn test_pending_approval_denied() {
    let op_key = "deny_test:run_command:unique456".to_string();
    let rx = park_approval(op_key.clone()).await;
    resolve_approval(&op_key, false).await;
    let approved = rx.await.unwrap();
    assert!(!approved);
}
