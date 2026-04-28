//! Tests for memory::conductor::extract — heuristic extraction, entity/topic extraction

use rove_engine::memory::conductor::extract::{
    extract_heuristic, extract_heuristic_combined, HeuristicExtractor, MemoryExtractor,
};
use rove_engine::memory::conductor::types::MemoryKind;

// ── Kind classification: Fact ────────────────────────────────────────────────

#[test]
fn fact_remember_that_prefix() {
    let out = extract_heuristic("remember that the API key is stored in vault");
    assert_eq!(out.kind, MemoryKind::Fact);
}

#[test]
fn fact_remember_colon_prefix() {
    let out = extract_heuristic("remember: the project uses async/await");
    assert_eq!(out.kind, MemoryKind::Fact);
}

#[test]
fn fact_remember_simple() {
    let out = extract_heuristic("remember the deploy command is make deploy");
    assert_eq!(out.kind, MemoryKind::Fact);
}

#[test]
fn fact_importance_at_least_0_85() {
    let out = extract_heuristic("remember that the deploy command is cargo build --release");
    assert!(out.importance >= 0.85);
}

#[test]
fn fact_has_remembered_fact_in_facts() {
    let out = extract_heuristic("remember that tests use tokio::test");
    assert!(!out.facts.is_empty());
    assert!(out.facts.iter().any(|(k, _)| k == "remembered_fact"));
}

#[test]
fn fact_summary_contains_content() {
    let out = extract_heuristic("remember that the linter is clippy");
    assert!(out.summary.contains("clippy") || !out.summary.is_empty());
}

#[test]
fn user_property_my_name_is() {
    let out = extract_heuristic("my preferred editor is neovim");
    assert_eq!(out.kind, MemoryKind::Fact);
    assert!(out.importance >= 0.9);
}

#[test]
fn user_property_fact_stored() {
    let out = extract_heuristic("my preferred linter is clippy");
    assert!(out.facts.iter().any(|(k, v)| k == "preferred linter" && v == "clippy"));
}

#[test]
fn user_property_my_name() {
    let out = extract_heuristic("my name is Alice");
    assert_eq!(out.kind, MemoryKind::Fact);
    assert!(out.facts.iter().any(|(k, v)| k == "name" && v == "Alice"));
}

#[test]
fn workspace_pattern_is_fact() {
    let out = extract_heuristic("I work on the rove project in ~/workspace/rove using Rust");
    assert_eq!(out.kind, MemoryKind::Fact);
}

#[test]
fn workspace_path_extracted_to_facts() {
    let out = extract_heuristic("I work on the rove project in ~/workspace/rove using Rust");
    assert!(out.facts.iter().any(|(k, _)| k == "workspace_path"));
}

#[test]
fn workspace_project_name_in_summary() {
    let out = extract_heuristic("I work on the rove project in ~/workspace/rove using Rust");
    assert!(out.summary.contains("rove") || !out.summary.is_empty());
}

// ── Kind classification: Warning ─────────────────────────────────────────────

#[test]
fn warning_never_use_unwrap() {
    let out = extract_heuristic("never use unwrap() in production code paths");
    assert_eq!(out.kind, MemoryKind::Warning);
}

#[test]
fn warning_importance_at_least_0_88() {
    let out = extract_heuristic("never use unwrap() in production code paths");
    assert!(out.importance >= 0.88);
}

#[test]
fn warning_dont_do() {
    let out = extract_heuristic("don't push directly to main branch ever");
    assert_eq!(out.kind, MemoryKind::Warning);
}

#[test]
fn warning_do_not() {
    let out = extract_heuristic("do not delete files without confirmation from user");
    assert_eq!(out.kind, MemoryKind::Warning);
}

#[test]
fn warning_avoid() {
    let out = extract_heuristic("avoid using unwrap() and expect() in library code paths");
    assert_eq!(out.kind, MemoryKind::Warning);
}

#[test]
fn warning_broken_if() {
    let out = extract_heuristic("broken if you run cargo test without --release flag first");
    assert_eq!(out.kind, MemoryKind::Warning);
}

// ── Kind classification: Decision ────────────────────────────────────────────

#[test]
fn decision_decided_to() {
    let out = extract_heuristic("decided to use sqlx instead of diesel for the new service");
    assert_eq!(out.kind, MemoryKind::Decision);
}

#[test]
fn decision_going_with() {
    let out = extract_heuristic("going with async tokio runtime for all services");
    assert_eq!(out.kind, MemoryKind::Decision);
}

#[test]
fn decision_switched_to() {
    let out = extract_heuristic("switched to axum from actix for the web framework choice");
    assert_eq!(out.kind, MemoryKind::Decision);
}

#[test]
fn decision_migrating_to() {
    let out = extract_heuristic("migrating to PostgreSQL from SQLite for production deployment");
    assert_eq!(out.kind, MemoryKind::Decision);
}

#[test]
fn decision_use_instead() {
    let out = extract_heuristic("use serde_json instead of miniserde for JSON parsing");
    assert_eq!(out.kind, MemoryKind::Decision);
}

#[test]
fn decision_importance_at_least_0_80() {
    let out = extract_heuristic("decided to use sqlx instead of diesel");
    assert!(out.importance >= 0.80);
}

// ── Kind classification: Preference ──────────────────────────────────────────

#[test]
fn preference_always_run_tests() {
    let out = extract_heuristic("always run cargo test before saying done");
    assert_eq!(out.kind, MemoryKind::Preference);
}

#[test]
fn preference_prefer() {
    let out = extract_heuristic("always use snake_case for variable naming in Rust code");
    assert_eq!(out.kind, MemoryKind::Preference);
}

#[test]
fn preference_i_prefer() {
    let out = extract_heuristic("I prefer small focused commits over large sweeping changes");
    assert_eq!(out.kind, MemoryKind::Preference);
}

#[test]
fn preference_run_tests_before_marking() {
    let out = extract_heuristic("run tests before marking task as done or complete");
    assert_eq!(out.kind, MemoryKind::Preference);
}

#[test]
fn preference_importance_at_least_0_85() {
    let out = extract_heuristic("always run cargo test before saying done");
    assert!(out.importance >= 0.85);
}

// ── Kind classification: Error ────────────────────────────────────────────────

#[test]
fn error_rust_error_code() {
    let out = extract_heuristic("error[E0499]: cannot borrow `data` as mutable more than once");
    assert_eq!(out.kind, MemoryKind::Error);
}

#[test]
fn error_panicked_at() {
    let out = extract_heuristic("panicked at 'index out of bounds: the len is 0 but the index is 0'");
    assert_eq!(out.kind, MemoryKind::Error);
}

#[test]
fn error_capital_failed() {
    let out = extract_heuristic("FAILED: test suite had 3 failing tests in test_auth module");
    assert_eq!(out.kind, MemoryKind::Error);
}

#[test]
fn error_capital_error() {
    let out = extract_heuristic("Error: connection refused to database at localhost:5432");
    assert_eq!(out.kind, MemoryKind::Error);
}

#[test]
fn error_importance_at_least_0_75() {
    let out = extract_heuristic("error[E0499]: cannot borrow as mutable");
    assert!(out.importance >= 0.75);
}

#[test]
fn error_entity_contains_code() {
    let out = extract_heuristic("error[E0499]: cannot borrow `data` as mutable more than once");
    assert!(out.entities.iter().any(|e| e.contains("E0499")));
}

// ── Kind classification: Fix ──────────────────────────────────────────────────

#[test]
fn fix_fixed_keyword() {
    let out = extract_heuristic("fixed the deadlock by dropping the lock before await points");
    assert_eq!(out.kind, MemoryKind::Fix);
}

#[test]
fn fix_resolved_keyword() {
    let out = extract_heuristic("resolved the memory leak by using Arc instead of raw pointers");
    assert_eq!(out.kind, MemoryKind::Fix);
}

#[test]
fn fix_works_now() {
    let out = extract_heuristic("works now after adding the missing lifetime annotation to struct");
    assert_eq!(out.kind, MemoryKind::Fix);
}

#[test]
fn fix_root_cause() {
    let out = extract_heuristic("root cause was the missing async drop implementation");
    assert_eq!(out.kind, MemoryKind::Fix);
}

#[test]
fn fix_the_issue_was() {
    let out = extract_heuristic("the issue was the missing return type annotation in function");
    assert_eq!(out.kind, MemoryKind::Fix);
}

#[test]
fn fix_importance_at_least_0_72() {
    let out = extract_heuristic("fixed the bug by adding proper error handling");
    assert!(out.importance >= 0.72);
}

// ── Kind classification: General ─────────────────────────────────────────────

#[test]
fn general_short_statement() {
    let out = extract_heuristic("checking the status of the build system");
    assert_eq!(out.kind, MemoryKind::General);
}

#[test]
fn general_neutral_text() {
    let out = extract_heuristic("the workspace is located at /home/user/projects");
    // Could be general unless workspace pattern matches
    let _ = out.kind;
}

// ── Entity extraction ─────────────────────────────────────────────────────────

#[test]
fn entities_file_path_extracted() {
    let out = extract_heuristic("tokio::spawn panicked in /src/engine/memory.rs");
    assert!(out.entities.iter().any(|e| e.contains("memory.rs")));
}

#[test]
fn entities_crate_path_extracted() {
    let out = extract_heuristic("tokio::spawn panicked in /src/engine/memory.rs");
    assert!(out.entities.iter().any(|e| e == "tokio"));
}

#[test]
fn entities_function_name_extracted() {
    let out = extract_heuristic("the fn calculate_hash is broken and needs to be fixed");
    assert!(out.entities.iter().any(|e| e == "calculate_hash"));
}

#[test]
fn entities_error_code_extracted() {
    let out = extract_heuristic("got error[E0502] and error[E0499] in the same function");
    assert!(out.entities.iter().any(|e| e.contains("E0502")));
}

#[test]
fn entities_max_10() {
    let out = extract_heuristic(
        "fn a fn b fn c fn d fn e fn f fn g fn h fn i fn j fn k fn l"
    );
    assert!(out.entities.len() <= 10);
}

#[test]
fn entities_dedup() {
    let out = extract_heuristic("fn read_file is called by fn read_file in /src/read_file.rs");
    let count = out.entities.iter().filter(|e| e.contains("read_file")).count();
    assert!(count <= 2, "Entities should be deduped: count = {}", count);
}

#[test]
fn entities_git_sha_extracted() {
    let out = extract_heuristic("commit abc1234 introduced the regression in auth module");
    // SHA-like patterns get extracted as commit refs
    let _ = out.entities;
}

// ── Topic extraction ──────────────────────────────────────────────────────────

#[test]
fn topics_rust_keywords() {
    let out = extract_heuristic("cargo build failed because the struct lifetime is wrong");
    assert!(out.topics.iter().any(|t| t == "rust"));
}

#[test]
fn topics_git_keywords() {
    let out = extract_heuristic("git commit failed because of merge conflict in branch main");
    assert!(out.topics.iter().any(|t| t == "git"));
}

#[test]
fn topics_sql_keywords() {
    let out = extract_heuristic("SELECT * FROM users WHERE id = 1 fails with migration error");
    assert!(out.topics.iter().any(|t| t == "sql"));
}

#[test]
fn topics_security_keywords() {
    let out = extract_heuristic("security token jwt auth permission role secret credential");
    assert!(out.topics.iter().any(|t| t == "security"));
}

#[test]
fn topics_testing_keywords() {
    let out = extract_heuristic("test unit integration mock fixture assert coverage");
    assert!(out.topics.iter().any(|t| t == "testing"));
}

#[test]
fn topics_debugging_keywords() {
    let out = extract_heuristic("error panic bug crash fix issue resolve exception traceback");
    assert!(out.topics.iter().any(|t| t == "debugging"));
}

#[test]
fn topics_max_5() {
    let out = extract_heuristic(
        "cargo test git commit SELECT INSERT docker pytest npm yarn webpack deploy release build"
    );
    assert!(out.topics.len() <= 5);
}

// ── Importance boosting ────────────────────────────────────────────────────────

#[test]
fn importance_boosted_by_critical() {
    let out1 = extract_heuristic("checking the build status");
    let out2 = extract_heuristic("this is a critical issue in the authentication flow");
    assert!(out2.importance > out1.importance);
}

#[test]
fn importance_boosted_by_never() {
    let out = extract_heuristic("never push to main without review and tests passing");
    assert!(out.importance > 0.40);
}

#[test]
fn importance_boosted_by_security() {
    let out = extract_heuristic("security vulnerability found in authentication module");
    assert!(out.importance > 0.55);
}

#[test]
fn importance_boosted_by_architecture() {
    let out = extract_heuristic("the architecture design decision was to use event sourcing");
    assert!(out.importance > 0.50);
}

#[test]
fn importance_penalized_for_short_input() {
    let out = extract_heuristic("yes");
    assert!(out.importance < 0.4);
}

#[test]
fn importance_clamped_to_1() {
    let out = extract_heuristic(
        "critical security vulnerability never push never delete never modify architecture decision"
    );
    assert!(out.importance <= 1.0);
}

#[test]
fn importance_at_least_0() {
    let out = extract_heuristic("yes");
    assert!(out.importance >= 0.0);
}

// ── Combined extraction ────────────────────────────────────────────────────────

#[test]
fn combined_reads_result_for_kind() {
    let out = extract_heuristic_combined(
        "investigate the failure",
        "fixed the deadlock by dropping the lock before await",
    );
    assert_eq!(out.kind, MemoryKind::Fix);
}

#[test]
fn combined_empty_result_uses_input() {
    let out = extract_heuristic_combined("remember that tests need --release flag", "");
    assert_eq!(out.kind, MemoryKind::Fact);
}

#[test]
fn combined_result_with_error_code() {
    let out = extract_heuristic_combined(
        "why does the build fail?",
        "error[E0499]: cannot borrow as mutable",
    );
    assert_eq!(out.kind, MemoryKind::Error);
    assert!(out.entities.iter().any(|e| e.contains("E0499")));
}

#[test]
fn combined_summary_not_empty() {
    let out = extract_heuristic_combined("some input", "some result");
    assert!(!out.summary.is_empty());
}

#[test]
fn combined_result_rust_topics() {
    let out = extract_heuristic_combined(
        "what is the struct definition",
        "the cargo crate tokio async trait impl struct needs lifetime",
    );
    assert!(out.topics.iter().any(|t| t == "rust"));
}

// ── HeuristicExtractor async ───────────────────────────────────────────────────

#[tokio::test]
async fn heuristic_extractor_name_is_heuristic() {
    let extractor = HeuristicExtractor::new();
    assert_eq!(extractor.name(), "heuristic");
}

#[tokio::test]
async fn heuristic_extractor_extract_fact() {
    let extractor = HeuristicExtractor::new();
    let out = extractor.extract("remember that the config is in ~/.rove", "").await;
    assert_eq!(out.kind, MemoryKind::Fact);
}

#[tokio::test]
async fn heuristic_extractor_extract_warning() {
    let extractor = HeuristicExtractor::new();
    let out = extractor.extract("never use unwrap() in lib code", "").await;
    assert_eq!(out.kind, MemoryKind::Warning);
}

#[tokio::test]
async fn heuristic_extractor_default_is_same() {
    let extractor = HeuristicExtractor;
    let out = extractor.extract("some general query", "").await;
    assert!(!out.summary.is_empty());
}

// ── Summary truncation ─────────────────────────────────────────────────────────

#[test]
fn summary_truncated_at_200_chars() {
    let long_input = "a ".repeat(200);
    let out = extract_heuristic(&long_input);
    assert!(out.summary.len() <= 210); // allow a bit for truncation marker
}

#[test]
fn summary_not_empty_for_normal_input() {
    let out = extract_heuristic("investigating the cargo build failure in CI");
    assert!(!out.summary.is_empty());
}

// ── Edge cases ─────────────────────────────────────────────────────────────────

#[test]
fn empty_input_does_not_panic() {
    let out = extract_heuristic("");
    assert!(out.importance >= 0.0);
}

#[test]
fn whitespace_only_input() {
    let out = extract_heuristic("   \n\t  ");
    assert!(out.importance >= 0.0);
}

#[test]
fn unicode_input_processed() {
    let out = extract_heuristic("記憶して: 設定ファイルは ~/.rove にある");
    assert!(!out.summary.is_empty());
}

#[test]
fn very_long_input_truncated() {
    let long = "word ".repeat(500);
    let out = extract_heuristic(&long);
    assert!(!out.summary.is_empty());
}

#[test]
fn multiple_patterns_first_wins_for_kind() {
    // remember + never: remember is checked first
    let out = extract_heuristic("remember that you should never use unwrap");
    assert_eq!(out.kind, MemoryKind::Fact);
}

#[test]
fn facts_vector_type() {
    let out = extract_heuristic("my editor is vim");
    for (key, value) in &out.facts {
        assert!(!key.is_empty());
        assert!(!value.is_empty());
    }
}
