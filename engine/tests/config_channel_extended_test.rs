//! Extended tests for config::channel — Channel parse edge cases, roundtrips, Display

use rove_engine::config::Channel;

// ── parse: dev aliases ────────────────────────────────────────────────────────

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

// ── parse: stable aliases ─────────────────────────────────────────────────────

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

// ── parse: None cases ─────────────────────────────────────────────────────────

#[test]
fn parse_empty_returns_none() {
    assert!(Channel::parse("").is_none());
}

#[test]
fn parse_unknown_returns_none() {
    assert!(Channel::parse("bogus").is_none());
}

#[test]
fn parse_canary_returns_none() {
    assert!(Channel::parse("canary").is_none());
}

#[test]
fn parse_beta_returns_none() {
    assert!(Channel::parse("beta").is_none());
}

#[test]
fn parse_alpha_returns_none() {
    assert!(Channel::parse("alpha").is_none());
}

#[test]
fn parse_whitespace_only_returns_none() {
    assert!(Channel::parse("   ").is_none());
}

#[test]
fn parse_numeric_returns_none() {
    assert!(Channel::parse("1.0").is_none());
}

// ── as_str ────────────────────────────────────────────────────────────────────

#[test]
fn stable_as_str_is_stable() {
    assert_eq!(Channel::Stable.as_str(), "stable");
}

#[test]
fn dev_as_str_is_dev() {
    assert_eq!(Channel::Dev.as_str(), "dev");
}

// ── Display ───────────────────────────────────────────────────────────────────

#[test]
fn stable_display() {
    assert_eq!(format!("{}", Channel::Stable), "stable");
}

#[test]
fn dev_display() {
    assert_eq!(format!("{}", Channel::Dev), "dev");
}

// ── Debug ─────────────────────────────────────────────────────────────────────

#[test]
fn stable_debug() {
    let s = format!("{:?}", Channel::Stable);
    assert!(s.contains("Stable"));
}

#[test]
fn dev_debug() {
    let s = format!("{:?}", Channel::Dev);
    assert!(s.contains("Dev"));
}

// ── Equality ─────────────────────────────────────────────────────────────────

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

// ── Copy ─────────────────────────────────────────────────────────────────────

#[test]
fn channel_copy_stable() {
    let c = Channel::Stable;
    let c2 = c;
    assert_eq!(c, c2);
}

#[test]
fn channel_copy_dev() {
    let c = Channel::Dev;
    let c2 = c;
    assert_eq!(c, c2);
}

// ── build_channel ─────────────────────────────────────────────────────────────

#[test]
fn build_channel_returns_valid_channel() {
    let c = Channel::build_channel();
    // Should be one of the known variants
    assert!(c == Channel::Stable || c == Channel::Dev);
}

// ── current() with env override ───────────────────────────────────────────────

#[test]
fn current_env_dev_overrides_build() {
    // Temporarily set env var
    std::env::set_var("ROVE_CHANNEL", "dev");
    let c = Channel::current();
    std::env::remove_var("ROVE_CHANNEL");
    assert_eq!(c, Channel::Dev);
}

#[test]
fn current_env_stable_overrides_build() {
    std::env::set_var("ROVE_CHANNEL", "stable");
    let c = Channel::current();
    std::env::remove_var("ROVE_CHANNEL");
    assert_eq!(c, Channel::Stable);
}

#[test]
fn current_env_nightly_overrides_to_dev() {
    std::env::set_var("ROVE_CHANNEL", "nightly");
    let c = Channel::current();
    std::env::remove_var("ROVE_CHANNEL");
    assert_eq!(c, Channel::Dev);
}

#[test]
fn current_env_release_overrides_to_stable() {
    std::env::set_var("ROVE_CHANNEL", "release");
    let c = Channel::current();
    std::env::remove_var("ROVE_CHANNEL");
    assert_eq!(c, Channel::Stable);
}

#[test]
fn current_invalid_env_falls_back_to_build() {
    std::env::set_var("ROVE_CHANNEL", "bogus");
    let c = Channel::current();
    std::env::remove_var("ROVE_CHANNEL");
    // Should fall back to build channel
    let build = Channel::build_channel();
    assert_eq!(c, build);
}

// ── Roundtrip: parse(as_str()) ────────────────────────────────────────────────

#[test]
fn stable_roundtrip_as_str_parse() {
    let c = Channel::Stable;
    assert_eq!(Channel::parse(c.as_str()), Some(Channel::Stable));
}

#[test]
fn dev_roundtrip_as_str_parse() {
    let c = Channel::Dev;
    assert_eq!(Channel::parse(c.as_str()), Some(Channel::Dev));
}

#[test]
fn stable_roundtrip_display_parse() {
    let displayed = format!("{}", Channel::Stable);
    assert_eq!(Channel::parse(&displayed), Some(Channel::Stable));
}

#[test]
fn dev_roundtrip_display_parse() {
    let displayed = format!("{}", Channel::Dev);
    assert_eq!(Channel::parse(&displayed), Some(Channel::Dev));
}

// ── parse with leading/trailing whitespace ────────────────────────────────────

#[test]
fn parse_dev_with_surrounding_whitespace() {
    assert_eq!(Channel::parse("  dev  "), Some(Channel::Dev));
}

#[test]
fn parse_stable_with_surrounding_whitespace() {
    assert_eq!(Channel::parse("  stable  "), Some(Channel::Stable));
}

#[test]
fn parse_nightly_with_leading_whitespace() {
    assert_eq!(Channel::parse(" nightly"), Some(Channel::Dev));
}

#[test]
fn parse_release_with_trailing_whitespace() {
    assert_eq!(Channel::parse("release "), Some(Channel::Stable));
}
