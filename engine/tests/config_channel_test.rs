//! Tests for config::channel — Channel enum, parse(), as_str(), Display

use rove_engine::config::Channel;

// ── Channel::parse() ──────────────────────────────────────────────────────────

#[test]
fn parse_dev_lowercase() {
    assert_eq!(Channel::parse("dev"), Some(Channel::Dev));
}

#[test]
fn parse_dev_uppercase() {
    assert_eq!(Channel::parse("DEV"), Some(Channel::Dev));
}

#[test]
fn parse_dev_mixed_case() {
    assert_eq!(Channel::parse("Dev"), Some(Channel::Dev));
}

#[test]
fn parse_nightly_lowercase() {
    assert_eq!(Channel::parse("nightly"), Some(Channel::Dev));
}

#[test]
fn parse_nightly_uppercase() {
    assert_eq!(Channel::parse("NIGHTLY"), Some(Channel::Dev));
}

#[test]
fn parse_nightly_mixed_case() {
    assert_eq!(Channel::parse("Nightly"), Some(Channel::Dev));
}

#[test]
fn parse_stable_lowercase() {
    assert_eq!(Channel::parse("stable"), Some(Channel::Stable));
}

#[test]
fn parse_stable_uppercase() {
    assert_eq!(Channel::parse("STABLE"), Some(Channel::Stable));
}

#[test]
fn parse_stable_mixed_case() {
    assert_eq!(Channel::parse("Stable"), Some(Channel::Stable));
}

#[test]
fn parse_release_lowercase() {
    assert_eq!(Channel::parse("release"), Some(Channel::Stable));
}

#[test]
fn parse_release_uppercase() {
    assert_eq!(Channel::parse("RELEASE"), Some(Channel::Stable));
}

#[test]
fn parse_release_mixed_case() {
    assert_eq!(Channel::parse("Release"), Some(Channel::Stable));
}

#[test]
fn parse_bogus_returns_none() {
    assert_eq!(Channel::parse("bogus"), None);
}

#[test]
fn parse_empty_string_returns_none() {
    assert_eq!(Channel::parse(""), None);
}

#[test]
fn parse_whitespace_returns_none() {
    assert_eq!(Channel::parse("  "), None);
}

#[test]
fn parse_alpha_returns_none() {
    assert_eq!(Channel::parse("beta"), None);
}

#[test]
fn parse_version_number_returns_none() {
    assert_eq!(Channel::parse("1.0.0"), None);
}

#[test]
fn parse_leading_whitespace_dev() {
    // trim() is applied so leading whitespace should be handled
    assert_eq!(Channel::parse("  dev"), Some(Channel::Dev));
}

#[test]
fn parse_trailing_whitespace_stable() {
    assert_eq!(Channel::parse("stable  "), Some(Channel::Stable));
}

// ── Channel::as_str() ─────────────────────────────────────────────────────────

#[test]
fn as_str_stable_is_stable() {
    assert_eq!(Channel::Stable.as_str(), "stable");
}

#[test]
fn as_str_dev_is_dev() {
    assert_eq!(Channel::Dev.as_str(), "dev");
}

#[test]
fn as_str_stable_not_release() {
    assert_ne!(Channel::Stable.as_str(), "release");
}

#[test]
fn as_str_dev_not_nightly() {
    assert_ne!(Channel::Dev.as_str(), "nightly");
}

// ── Channel Display ───────────────────────────────────────────────────────────

#[test]
fn display_stable() {
    assert_eq!(format!("{}", Channel::Stable), "stable");
}

#[test]
fn display_dev() {
    assert_eq!(format!("{}", Channel::Dev), "dev");
}

#[test]
fn display_matches_as_str_stable() {
    let ch = Channel::Stable;
    assert_eq!(format!("{}", ch), ch.as_str());
}

#[test]
fn display_matches_as_str_dev() {
    let ch = Channel::Dev;
    assert_eq!(format!("{}", ch), ch.as_str());
}

// ── Channel equality ──────────────────────────────────────────────────────────

#[test]
fn stable_eq_stable() {
    assert_eq!(Channel::Stable, Channel::Stable);
}

#[test]
fn dev_eq_dev() {
    assert_eq!(Channel::Dev, Channel::Dev);
}

#[test]
fn stable_ne_dev() {
    assert_ne!(Channel::Stable, Channel::Dev);
}

#[test]
fn dev_ne_stable() {
    assert_ne!(Channel::Dev, Channel::Stable);
}

// ── Channel::current() ────────────────────────────────────────────────────────

#[test]
fn current_returns_a_channel() {
    let ch = Channel::current();
    // Must be either Stable or Dev
    assert!(ch == Channel::Stable || ch == Channel::Dev);
}

#[test]
fn current_env_override_dev() {
    // Set env var and check
    std::env::set_var("ROVE_CHANNEL", "dev");
    let ch = Channel::current();
    std::env::remove_var("ROVE_CHANNEL");
    assert_eq!(ch, Channel::Dev);
}

#[test]
fn current_env_override_stable() {
    std::env::set_var("ROVE_CHANNEL", "stable");
    let ch = Channel::current();
    std::env::remove_var("ROVE_CHANNEL");
    assert_eq!(ch, Channel::Stable);
}

#[test]
fn current_env_override_nightly_becomes_dev() {
    std::env::set_var("ROVE_CHANNEL", "nightly");
    let ch = Channel::current();
    std::env::remove_var("ROVE_CHANNEL");
    assert_eq!(ch, Channel::Dev);
}

#[test]
fn current_env_override_release_becomes_stable() {
    std::env::set_var("ROVE_CHANNEL", "release");
    let ch = Channel::current();
    std::env::remove_var("ROVE_CHANNEL");
    assert_eq!(ch, Channel::Stable);
}

#[test]
fn current_invalid_env_falls_back_to_build_channel() {
    std::env::set_var("ROVE_CHANNEL", "invalid_channel_xyz");
    let ch = Channel::current();
    std::env::remove_var("ROVE_CHANNEL");
    // Should fall back to build channel, which is either Stable or Dev
    assert!(ch == Channel::Stable || ch == Channel::Dev);
}

#[test]
fn current_empty_env_falls_back_to_build_channel() {
    std::env::set_var("ROVE_CHANNEL", "");
    let ch = Channel::current();
    std::env::remove_var("ROVE_CHANNEL");
    assert!(ch == Channel::Stable || ch == Channel::Dev);
}

// ── Channel::build_channel() ──────────────────────────────────────────────────

#[test]
fn build_channel_returns_valid_channel() {
    let ch = Channel::build_channel();
    assert!(ch == Channel::Stable || ch == Channel::Dev);
}

#[test]
fn build_channel_is_stable_or_dev() {
    let ch = Channel::build_channel();
    let s = ch.as_str();
    assert!(s == "stable" || s == "dev");
}

// ── Round-trip ────────────────────────────────────────────────────────────────

#[test]
fn roundtrip_stable_as_str_parse() {
    let ch = Channel::Stable;
    let parsed = Channel::parse(ch.as_str()).unwrap();
    assert_eq!(parsed, ch);
}

#[test]
fn roundtrip_dev_as_str_parse() {
    let ch = Channel::Dev;
    let parsed = Channel::parse(ch.as_str()).unwrap();
    assert_eq!(parsed, ch);
}

// ── Copy/Clone ────────────────────────────────────────────────────────────────

#[test]
fn channel_copy_stable() {
    let ch = Channel::Stable;
    let ch2 = ch;
    assert_eq!(ch, ch2);
}

#[test]
fn channel_copy_dev() {
    let ch = Channel::Dev;
    let ch2 = ch;
    assert_eq!(ch, ch2);
}

#[test]
fn channel_debug_stable() {
    let s = format!("{:?}", Channel::Stable);
    assert!(s.contains("Stable"));
}

#[test]
fn channel_debug_dev() {
    let s = format!("{:?}", Channel::Dev);
    assert!(s.contains("Dev"));
}
