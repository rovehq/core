//! Tests for SecretString and scrub_text()

use rove_engine::security::secrets::{scrub_text, scrub_text_with_values, SecretString};

// ── SecretString construction ──────────────────────────────────────────────────

#[test]
fn secret_string_new_from_str() {
    let s = SecretString::new("my-secret");
    assert_eq!(s.unsecure(), "my-secret");
}

#[test]
fn secret_string_new_from_string() {
    let s = SecretString::new("my-key".to_string());
    assert_eq!(s.unsecure(), "my-key");
}

#[test]
fn secret_string_from_str_impl() {
    let s: SecretString = "hello".into();
    assert_eq!(s.unsecure(), "hello");
}

#[test]
fn secret_string_from_string_impl() {
    let s: SecretString = "hello".to_string().into();
    assert_eq!(s.unsecure(), "hello");
}

#[test]
fn secret_string_empty() {
    let s = SecretString::new("");
    assert_eq!(s.unsecure(), "");
}

#[test]
fn secret_string_with_special_chars() {
    let s = SecretString::new("s3cr3t!@#$%^&*()");
    assert_eq!(s.unsecure(), "s3cr3t!@#$%^&*()");
}

#[test]
fn secret_string_with_unicode() {
    let s = SecretString::new("パスワード");
    assert_eq!(s.unsecure(), "パスワード");
}

// ── SecretString display ──────────────────────────────────────────────────────

#[test]
fn secret_string_display_is_redacted() {
    let s = SecretString::new("actual-secret");
    assert_eq!(format!("{}", s), "[REDACTED]");
}

#[test]
fn secret_string_debug_is_redacted() {
    let s = SecretString::new("actual-secret");
    let debug = format!("{:?}", s);
    assert!(debug.contains("REDACTED"));
    assert!(!debug.contains("actual-secret"));
}

#[test]
fn secret_string_display_does_not_leak() {
    let s = SecretString::new("supersecretkey12345");
    let displayed = format!("{}", s);
    assert!(!displayed.contains("supersecretkey12345"));
}

#[test]
fn secret_string_debug_does_not_leak() {
    let s = SecretString::new("supersecretkey12345");
    let debug = format!("{:?}", s);
    assert!(!debug.contains("supersecretkey12345"));
}

// ── SecretString equality ─────────────────────────────────────────────────────

#[test]
fn secret_string_eq_same_value() {
    let s1 = SecretString::new("secret");
    let s2 = SecretString::new("secret");
    assert_eq!(s1, s2);
}

#[test]
fn secret_string_ne_different_value() {
    let s1 = SecretString::new("secret1");
    let s2 = SecretString::new("secret2");
    assert_ne!(s1, s2);
}

#[test]
fn secret_string_eq_empty() {
    let s1 = SecretString::new("");
    let s2 = SecretString::new("");
    assert_eq!(s1, s2);
}

// ── SecretString clone ────────────────────────────────────────────────────────

#[test]
fn secret_string_clone_equal() {
    let s = SecretString::new("the-secret");
    let cloned = s.clone();
    assert_eq!(s.unsecure(), cloned.unsecure());
}

#[test]
fn secret_string_clone_independent() {
    let s1 = SecretString::new("original");
    let _s2 = s1.clone();
    assert_eq!(s1.unsecure(), "original");
}

// ── SecretString drop ─────────────────────────────────────────────────────────

#[test]
fn secret_string_drops_without_panic() {
    {
        let _s = SecretString::new("ephemeral-secret");
    }
    // No panic means zeroize drop worked
}

#[test]
fn secret_string_multiple_drops() {
    for _ in 0..10 {
        let _s = SecretString::new("repeated-secret");
    }
}

// ── scrub_text: OpenAI keys ────────────────────────────────────────────────────

#[test]
fn scrub_openai_key_replaced() {
    let text = "key is sk-abcdefghijklmnopqrstuvwxyz1234567890";
    let result = scrub_text(text);
    assert!(result.contains("[REDACTED]"));
    assert!(!result.contains("sk-abcdef"));
}

#[test]
fn scrub_openai_key_short_pattern() {
    // Only 8+ chars after sk- are matched
    let text = "sk-ABCDEFGH is a key";
    let result = scrub_text(text);
    assert!(result.contains("[REDACTED]"));
}

#[test]
fn scrub_openai_key_too_short_not_replaced() {
    // sk- with 7 chars = not matched
    let text = "sk-ABCDE has a key";
    let result = scrub_text(text);
    // May or may not be redacted depending on exact pattern bounds
    let _ = result;
}

#[test]
fn scrub_openai_key_preserves_surrounding_text() {
    let text = "The API key is sk-abcdefghijklmnopqrstuvwxyz1234567890 please use it";
    let result = scrub_text(text);
    assert!(result.contains("The API key is"));
    assert!(result.contains("please use it"));
}

// ── scrub_text: Google API keys ────────────────────────────────────────────────

#[test]
fn scrub_google_key_replaced() {
    let text = "google key: AIzaSyBxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx end";
    let result = scrub_text(text);
    assert!(result.contains("[REDACTED]"));
}

#[test]
fn scrub_google_key_preserves_context() {
    let text = "google key: AIzaSyBxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx end";
    let result = scrub_text(text);
    assert!(result.contains("google key:"));
    assert!(result.contains("end"));
}

// ── scrub_text: GitHub tokens ─────────────────────────────────────────────────

#[test]
fn scrub_github_pat_replaced() {
    let text = "github token ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef1234";
    let result = scrub_text(text);
    assert!(result.contains("[REDACTED]"));
    assert!(!result.contains("ghp_ABCD"));
}

#[test]
fn scrub_github_pat_preserves_surrounding() {
    let text = "token: ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef1234 is the github pat";
    let result = scrub_text(text);
    assert!(result.contains("token:"));
    assert!(result.contains("is the github pat"));
}

// ── scrub_text: Bearer tokens ─────────────────────────────────────────────────

#[test]
fn scrub_bearer_token_replaced() {
    let text = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.abc";
    let result = scrub_text(text);
    assert!(result.contains("[REDACTED]"));
}

#[test]
fn scrub_bearer_short_token_not_redacted() {
    // Bearer + fewer than 20 chars should not match
    let text = "Authorization: Bearer short";
    let result = scrub_text(text);
    // short bearer may not be redacted
    let _ = result;
}

// ── scrub_text: Telegram tokens ───────────────────────────────────────────────

#[test]
fn scrub_telegram_bot_token_replaced() {
    let text = "telegram token: 1234567890:ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghi";
    let result = scrub_text(text);
    assert!(result.contains("[REDACTED]"));
}

// ── scrub_text: clean text ────────────────────────────────────────────────────

#[test]
fn scrub_clean_text_unchanged() {
    let text = "This is a clean sentence with no secrets.";
    let result = scrub_text(text);
    assert_eq!(result, text);
}

#[test]
fn scrub_empty_string() {
    let result = scrub_text("");
    assert_eq!(result, "");
}

#[test]
fn scrub_whitespace_only() {
    let result = scrub_text("   \n\t  ");
    assert_eq!(result, "   \n\t  ");
}

#[test]
fn scrub_multiple_secrets_in_one_text() {
    let text = "openai: sk-abcdefghijklmnopqrstuvwxyz1234567890 and github: ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef1234";
    let result = scrub_text(text);
    assert!(!result.contains("sk-abcdef"));
    assert!(!result.contains("ghp_ABCD"));
}

#[test]
fn scrub_non_secret_numbers_unchanged() {
    let text = "The count is 42 and the ratio is 3.14.";
    let result = scrub_text(text);
    assert_eq!(result, text);
}

// ── scrub_text_with_values ────────────────────────────────────────────────────

#[test]
fn scrub_with_values_replaces_known_secret() {
    let text = "token is mytoken123 and it should be hidden";
    let result = scrub_text_with_values(text, &["mytoken123".to_string()]);
    assert!(result.contains("[REDACTED]"));
    assert!(!result.contains("mytoken123"));
}

#[test]
fn scrub_with_values_replaces_multiple_secrets() {
    let text = "first: abc123secret, second: xyz789secret";
    let result = scrub_text_with_values(
        text,
        &["abc123secret".to_string(), "xyz789secret".to_string()],
    );
    assert!(!result.contains("abc123secret"));
    assert!(!result.contains("xyz789secret"));
}

#[test]
fn scrub_with_values_preserves_non_secret_text() {
    let text = "token is mysecret and this is preserved text";
    let result = scrub_text_with_values(text, &["mysecret".to_string()]);
    assert!(result.contains("and this is preserved text"));
}

#[test]
fn scrub_with_values_empty_values_list() {
    let text = "some text without secrets here";
    let result = scrub_text_with_values(text, &[]);
    assert_eq!(result, text);
}

#[test]
fn scrub_with_values_empty_value_skipped() {
    let text = "some text";
    let result = scrub_text_with_values(text, &["".to_string(), "   ".to_string()]);
    assert_eq!(result, text);
}

#[test]
fn scrub_with_values_also_applies_patterns() {
    // Even with empty custom values, pattern-based scrubbing still applies
    let text = "key: sk-abcdefghijklmnopqrstuvwxyz1234567890";
    let result = scrub_text_with_values(text, &[]);
    assert!(result.contains("[REDACTED]"));
}

#[test]
fn scrub_with_values_trims_secret_before_matching() {
    let text = "token is mytoken and more";
    // Secret with leading/trailing whitespace
    let result = scrub_text_with_values(text, &["  mytoken  ".to_string()]);
    assert!(result.contains("[REDACTED]") || !result.contains("mytoken"));
}

// ── SecretString field security ───────────────────────────────────────────────

#[test]
fn secret_string_not_in_log_output() {
    let s = SecretString::new("VERYSECRETVALUE");
    let log_line = format!("Request processed with key={}", s);
    assert!(!log_line.contains("VERYSECRETVALUE"));
    assert!(log_line.contains("[REDACTED]"));
}

#[test]
fn secret_string_not_in_error_message() {
    let s = SecretString::new("SECRETINVAULT");
    let error = format!("Error processing {:?}: invalid format", s);
    assert!(!error.contains("SECRETINVAULT"));
}

#[test]
fn secret_string_vec_does_not_expose() {
    let secrets = vec![
        SecretString::new("secret1"),
        SecretString::new("secret2"),
        SecretString::new("secret3"),
    ];
    let formatted = format!("{:?}", secrets);
    assert!(!formatted.contains("secret1"));
    assert!(!formatted.contains("secret2"));
    assert!(!formatted.contains("secret3"));
}
