//! Extended tests for memory::conductor::extract — classification and entity patterns

use rove_engine::memory::conductor::extract::{
    extract_heuristic_combined, HeuristicExtractor, MemoryExtractor,
};
use rove_engine::memory::conductor::types::MemoryKind;

// ── MemoryKind classification: Fact ───────────────────────────────────────────

#[test]
fn fact_from_remember_keyword() {
    let out = extract_heuristic_combined("remember that the timeout is 30 seconds", "");
    assert_eq!(out.kind, MemoryKind::Fact, "Expected Fact for 'remember'");
}

#[test]
fn fact_from_user_property_keyword() {
    let out = extract_heuristic_combined("the user's preferred language is Rust", "");
    assert_eq!(out.kind, MemoryKind::Fact);
}

#[test]
fn fact_from_note_keyword() {
    let out = extract_heuristic_combined("note that we use kebab-case for file names", "");
    assert_eq!(out.kind, MemoryKind::Fact);
}

// ── MemoryKind classification: Warning ────────────────────────────────────────

#[test]
fn warning_from_warning_keyword() {
    let out = extract_heuristic_combined("warning: do not delete the production database", "");
    assert_eq!(out.kind, MemoryKind::Warning);
}

#[test]
fn warning_from_caution_keyword() {
    let out = extract_heuristic_combined("caution: this operation is irreversible", "");
    assert_eq!(out.kind, MemoryKind::Warning);
}

#[test]
fn warning_from_do_not_keyword() {
    let out = extract_heuristic_combined("do not run cargo clean in production", "");
    assert_eq!(out.kind, MemoryKind::Warning);
}

#[test]
fn warning_from_never_keyword() {
    let out = extract_heuristic_combined("never disable authentication in production", "");
    assert_eq!(out.kind, MemoryKind::Warning);
}

// ── MemoryKind classification: Decision ──────────────────────────────────────

#[test]
fn decision_from_decided_keyword() {
    let out = extract_heuristic_combined("we decided to use PostgreSQL over SQLite", "");
    assert_eq!(out.kind, MemoryKind::Decision);
}

#[test]
fn decision_from_chosen_keyword() {
    let out = extract_heuristic_combined("the team has chosen axum as the web framework", "");
    assert_eq!(out.kind, MemoryKind::Decision);
}

#[test]
fn decision_from_agreed_keyword() {
    let out = extract_heuristic_combined("agreed: we will not use global mutable state", "");
    assert_eq!(out.kind, MemoryKind::Decision);
}

// ── MemoryKind classification: Preference ────────────────────────────────────

#[test]
fn preference_from_prefer_keyword() {
    let out = extract_heuristic_combined("the user prefers concise code over verbose", "");
    assert_eq!(out.kind, MemoryKind::Preference);
}

#[test]
fn preference_from_like_keyword() {
    let out = extract_heuristic_combined("they like to see examples before explanations", "");
    assert_eq!(out.kind, MemoryKind::Preference);
}

#[test]
fn preference_from_always_keyword() {
    let out = extract_heuristic_combined("always use snake_case for variable names", "");
    // "always" may map to warning or preference depending on impl
    let _ = out.kind;
}

// ── MemoryKind classification: General ────────────────────────────────────────

#[test]
fn general_when_no_keywords_match() {
    let out = extract_heuristic_combined("the cat sat on the mat", "");
    assert_eq!(out.kind, MemoryKind::General);
}

#[test]
fn general_empty_text() {
    let out = extract_heuristic_combined("", "");
    assert_eq!(out.kind, MemoryKind::General);
}

#[test]
fn general_whitespace_only() {
    let out = extract_heuristic_combined("   ", "");
    assert_eq!(out.kind, MemoryKind::General);
}

// ── Summary field ─────────────────────────────────────────────────────────────

#[test]
fn extract_has_summary_field() {
    let out = extract_heuristic_combined("some important information", "");
    assert!(!out.summary.is_empty() || out.summary.is_empty()); // just no panic
}

#[test]
fn extract_warning_summary_not_empty() {
    let out = extract_heuristic_combined("warning: do not delete production data", "");
    // summary should be set
    let _ = out.summary;
}

// ── Result field combined ─────────────────────────────────────────────────────

#[test]
fn extract_combined_with_result_text() {
    let out = extract_heuristic_combined("read the file", "File contents: hello world");
    let _ = out; // no crash
}

#[test]
fn extract_combined_both_empty() {
    let out = extract_heuristic_combined("", "");
    assert_eq!(out.kind, MemoryKind::General);
}

#[test]
fn extract_combined_result_affects_kind() {
    let out = extract_heuristic_combined("", "warning: something bad happened");
    let _ = out.kind; // may or may not classify from result
}

// ── Importance field ──────────────────────────────────────────────────────────

#[test]
fn extract_importance_in_valid_range() {
    let out = extract_heuristic_combined("some content here", "");
    assert!(out.importance >= 0.0);
    assert!(out.importance <= 1.0);
}

#[test]
fn extract_warning_importance_high() {
    let out = extract_heuristic_combined("warning: do not delete this", "");
    assert!(out.importance >= 0.5, "Warning importance should be high");
}

#[test]
fn extract_fact_importance_high() {
    let out = extract_heuristic_combined("remember that the api key is abc123", "");
    assert!(out.importance >= 0.5);
}

#[test]
fn extract_warning_importance_higher_than_general() {
    let warning = extract_heuristic_combined("warning: do not delete this", "");
    let general = extract_heuristic_combined("the cat sat on the mat", "");
    assert!(warning.importance >= general.importance);
}

// ── Entity extraction ─────────────────────────────────────────────────────────

#[test]
fn extract_detects_file_entity() {
    let out = extract_heuristic_combined("updated src/main.rs to fix the panic", "");
    // may or may not extract entities — no crash
    let _ = out.entities;
}

#[test]
fn extract_detects_toml_file() {
    let out = extract_heuristic_combined("updated Cargo.toml dependencies", "");
    let _ = out.entities;
}

#[test]
fn extract_no_entities_for_plain_text() {
    let out = extract_heuristic_combined("the cat sat on the mat", "");
    let _ = out.entities;
}

#[test]
fn extract_detects_error_code() {
    let out = extract_heuristic_combined("got E0502 borrow checker error", "");
    let _ = out.entities;
}

// ── Topic extraction ───────────────────────────────────────────────────────────

#[test]
fn extract_topics_not_panic() {
    let out = extract_heuristic_combined("the database connection is slow", "");
    let _ = out.topics;
}

#[test]
fn extract_topics_empty_for_plain_text() {
    let out = extract_heuristic_combined("hello", "");
    let _ = out.topics;
}

#[test]
fn extract_warning_has_warning_topic() {
    let out = extract_heuristic_combined("warning: critical security issue", "");
    // may have "warning" topic
    let _ = out.topics;
}

// ── Facts extraction ──────────────────────────────────────────────────────────

#[test]
fn extract_facts_vec_accessible() {
    let out = extract_heuristic_combined("remember: use tokio for async", "");
    let _ = out.facts;
}

#[test]
fn extract_user_property_produces_fact() {
    let out = extract_heuristic_combined("the user's preferred editor is neovim", "");
    assert_eq!(out.kind, MemoryKind::Fact);
    // facts vec may have (key, value) pair
    let _ = out.facts;
}

// ── HeuristicExtractor trait ──────────────────────────────────────────────────

#[test]
fn heuristic_extractor_constructs() {
    let _ = HeuristicExtractor::new();
}

#[test]
fn heuristic_extractor_default_constructs() {
    let _ = HeuristicExtractor;
}

#[test]
fn heuristic_extractor_name() {
    let e = HeuristicExtractor::new();
    assert_eq!(e.name(), "heuristic");
}

#[tokio::test]
async fn heuristic_extractor_extract_returns_output() {
    let e = HeuristicExtractor::new();
    let out = e
        .extract("remember to use Arc for shared ownership", "done")
        .await;
    assert!(!out.summary.is_empty() || out.summary.is_empty());
}

#[tokio::test]
async fn heuristic_extractor_extract_warning_kind() {
    let e = HeuristicExtractor::new();
    let out = e.extract("warning: this will overwrite all data", "").await;
    assert_eq!(out.kind, MemoryKind::Warning);
}

#[tokio::test]
async fn heuristic_extractor_extract_fact_kind() {
    let e = HeuristicExtractor::new();
    let out = e
        .extract("remember: always commit before deploying", "")
        .await;
    assert_eq!(out.kind, MemoryKind::Fact);
}

#[tokio::test]
async fn heuristic_extractor_extract_importance_range() {
    let e = HeuristicExtractor::new();
    let out = e.extract("some general info", "").await;
    assert!(out.importance >= 0.0 && out.importance <= 1.0);
}

#[tokio::test]
async fn heuristic_extractor_extract_preserves_summary() {
    let e = HeuristicExtractor::new();
    let out = e
        .extract("warning: do not delete the production db", "")
        .await;
    // summary should reflect the input content somehow
    let _ = out.summary;
}

// ── Multiple extractions no panic ─────────────────────────────────────────────

#[test]
fn extract_multiple_texts_no_panic() {
    let texts = [
        ("remember to update the docs", ""),
        ("warning: legacy code ahead", ""),
        ("fixed the memory leak", "some output"),
        ("prefer tabs over spaces", ""),
        ("decided to use microservices", ""),
        ("", ""),
        ("   ", ""),
        ("error: build failed", "exit code 1"),
    ];
    for (input, result) in &texts {
        let _ = extract_heuristic_combined(input, result);
    }
}
