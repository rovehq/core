//! Memory Extraction Backends
//!
//! `MemoryExtractor` is a trait that abstracts over three strategies:
//!   - `HeuristicExtractor`  — pure regex/pattern matching, zero LLM deps
//!   - `LlmExtractor`        — calls LLMRouter, falls back to heuristic
//!   - `AutoExtractor`       — tries cloud → local → heuristic
//!
//! Which extractor to use is controlled by `MemoryConfig.extraction_backend`.
//! The default (`Auto`) works in every deployment without configuration.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use regex::Regex;
use tracing::warn;

use crate::conductor::types::MemoryKind;
use crate::llm::router::LLMRouter;

// ─────────────────────────────────────────────────────────────────────────────
// ExtractionOutput — returned by all extractors
// ─────────────────────────────────────────────────────────────────────────────

/// Structured output from a memory extraction pass.
#[derive(Debug, Clone)]
pub struct ExtractionOutput {
    /// One-sentence summary of what happened.
    pub summary: String,
    /// Extracted proper nouns, file paths, crate names, error codes.
    pub entities: Vec<String>,
    /// Abstract domain topics: "rust", "git", "security", etc.
    pub topics: Vec<String>,
    /// Importance score 0.0–1.0.
    pub importance: f64,
    /// Semantic kind — used for typed queries.
    pub kind: MemoryKind,
    /// Key-value facts extracted for the fact store.
    /// Each tuple is (key, value) — e.g. ("preferred_linter", "clippy").
    pub facts: Vec<(String, String)>,
}

// ─────────────────────────────────────────────────────────────────────────────
// MemoryExtractor trait
// ─────────────────────────────────────────────────────────────────────────────

#[async_trait]
pub trait MemoryExtractor: Send + Sync {
    /// Extract structured data from a completed task.
    ///
    /// `input`  — the user's original request / task input
    /// `result` — the agent's final answer or tool output
    async fn extract(&self, input: &str, result: &str) -> ExtractionOutput;

    /// Human-readable name for logging and config introspection.
    fn name(&self) -> &'static str;
}

// ─────────────────────────────────────────────────────────────────────────────
// HeuristicExtractor — zero LLM deps, always available
// ─────────────────────────────────────────────────────────────────────────────

pub struct HeuristicExtractor;

impl HeuristicExtractor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HeuristicExtractor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MemoryExtractor for HeuristicExtractor {
    async fn extract(&self, input: &str, result: &str) -> ExtractionOutput {
        extract_heuristic_combined(input, result)
    }

    fn name(&self) -> &'static str {
        "heuristic"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// LlmExtractor — wraps LLMRouter, falls back to heuristic on failure
// ─────────────────────────────────────────────────────────────────────────────

pub struct LlmExtractor {
    router: Arc<LLMRouter>,
}

impl LlmExtractor {
    pub fn new(router: Arc<LLMRouter>) -> Self {
        Self { router }
    }
}

#[async_trait]
impl MemoryExtractor for LlmExtractor {
    async fn extract(&self, input: &str, result: &str) -> ExtractionOutput {
        match extract_via_llm(&self.router, input, result).await {
            Ok(out) => out,
            Err(e) => {
                warn!(error = %e, "LLM extraction failed, falling back to heuristic");
                extract_heuristic_combined(input, result)
            }
        }
    }

    fn name(&self) -> &'static str {
        "llm"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// AutoExtractor — best available: cloud → local → heuristic
// ─────────────────────────────────────────────────────────────────────────────

pub struct AutoExtractor {
    router: Arc<LLMRouter>,
}

impl AutoExtractor {
    pub fn new(router: Arc<LLMRouter>) -> Self {
        Self { router }
    }
}

#[async_trait]
impl MemoryExtractor for AutoExtractor {
    async fn extract(&self, input: &str, result: &str) -> ExtractionOutput {
        // Try LLM (router handles cloud→local preference automatically).
        // If LLM is unavailable or times out, fall through to heuristic —
        // no memory is ever lost, just less enriched.
        match extract_via_llm(&self.router, input, result).await {
            Ok(out) => out,
            Err(_) => extract_heuristic_combined(input, result),
        }
    }

    fn name(&self) -> &'static str {
        "auto"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Heuristic extraction — 15+ patterns, no LLM
// ─────────────────────────────────────────────────────────────────────────────

pub fn extract_heuristic(input: &str) -> ExtractionOutput {
    extract_heuristic_combined(input, "")
}

pub fn extract_heuristic_combined(input: &str, result: &str) -> ExtractionOutput {
    let combined = if result.trim().is_empty() {
        input.to_string()
    } else {
        format!("{input}\n\nResult:\n{result}")
    };

    // Default summary is a truncated version of the input;
    // specific patterns below may replace it with a better extraction.
    let mut summary = crate::conductor::memory_utils::truncate(&combined, 200);
    let mut entities: Vec<String> = Vec::new();
    let mut topics: Vec<String> = Vec::new();
    let mut importance: f64 = 0.40;
    let mut kind = MemoryKind::General;
    let mut facts: Vec<(String, String)> = Vec::new();

    // ── Kind classification (first match wins) ──────────────────────────

    if let Some(fact) = match_remembered_fact(&combined) {
        summary = fact.clone();
        facts.push(("remembered_fact".to_string(), fact));
        kind = MemoryKind::Fact;
        importance = importance.max(0.85);
        push_topic(&mut topics, "remembered_fact");
    } else if let Some((key, value)) = match_user_property(&combined) {
        summary = format!("User's {} is {}.", key, value);
        facts.push((key, value));
        kind = MemoryKind::Fact;
        importance = importance.max(0.90);
        push_topic(&mut topics, "user_preference");
    } else if let Some(ws) = match_workspace(&combined) {
        summary = ws.clone();
        // Workspace statements are structured facts too
        if let Some(path) = extract_workspace_path(&combined) {
            facts.push(("workspace_path".to_string(), path));
        }
        kind = MemoryKind::Fact;
        importance = importance.max(0.80);
        push_topic(&mut topics, "project_context");
    } else if let Some(warn_text) = match_warning(&combined) {
        // Warnings before preferences: "never do X" is a warning, not a preference
        summary = warn_text;
        kind = MemoryKind::Warning;
        importance = importance.max(0.88);
        push_topic(&mut topics, "warning");
    } else if let Some(decision) = match_decision(&combined) {
        // Decisions before preferences: "decided to use X for Y" is a decision
        summary = decision;
        kind = MemoryKind::Decision;
        importance = importance.max(0.80);
        push_topic(&mut topics, "decision");
    } else if let Some(pref) = match_preference(&combined) {
        summary = pref;
        kind = MemoryKind::Preference;
        importance = importance.max(0.85);
        push_topic(&mut topics, "preference");
    } else if let Some(err) = match_error(&combined) {
        summary = err;
        kind = MemoryKind::Error;
        importance = importance.max(0.75);
        push_topic(&mut topics, "error");
    } else if let Some(fix) = match_fix(&combined) {
        summary = fix;
        kind = MemoryKind::Fix;
        importance = importance.max(0.72);
        push_topic(&mut topics, "fix");
    }

    // ── Entity extraction (always run, independent of kind) ─────────────

    entities.extend(extract_file_entities(&combined));
    entities.extend(extract_function_entities(&combined));
    entities.extend(extract_error_codes(&combined));
    entities.extend(extract_crate_paths(&combined));
    entities.extend(extract_git_refs(&combined));
    entities.dedup();
    entities.truncate(10);

    // ── Topic extraction from domain vocabulary ──────────────────────────

    topics.extend(topics_from_vocabulary(&combined));
    topics.dedup();
    topics.truncate(5);

    // ── Importance boosting ──────────────────────────────────────────────

    importance = boost_importance(&combined, importance).clamp(0.0, 1.0);

    ExtractionOutput {
        summary: if summary.is_empty() {
            crate::conductor::memory_utils::truncate(&combined, 200)
        } else {
            summary
        },
        entities,
        topics,
        importance,
        kind,
        facts,
    }
}

// ── Pattern matchers ─────────────────────────────────────────────────────────

/// "remember that X" / "remember X" / "remember: X"
fn match_remembered_fact(input: &str) -> Option<String> {
    let re = Regex::new(r"(?i)^\s*remember(?:\s+that|:)?\s+(.+?)\s*$").ok()?;
    let cap = re.captures(input.trim())?;
    let fact = cap.get(1)?.as_str().trim().trim_end_matches('.');
    if fact.is_empty() {
        return None;
    }
    Some(format!("Remembered fact: {}.", fact))
}

/// "my X is Y"
fn match_user_property(input: &str) -> Option<(String, String)> {
    let re = Regex::new(r"(?i)^\s*my\s+(.+?)\s+is\s+(.+?)\s*$").ok()?;
    let cap = re.captures(input.trim())?;
    let key = cap.get(1)?.as_str().trim().to_ascii_lowercase();
    let val = cap
        .get(2)?
        .as_str()
        .trim()
        .trim_end_matches('.')
        .to_string();
    if key.is_empty() || val.is_empty() {
        return None;
    }
    Some((key, val))
}

/// "I work on PROJECT in PATH using LANG"
fn match_workspace(input: &str) -> Option<String> {
    let re = Regex::new(
        r"(?i)^\s*i work on(?:\s+the)?\s+(.+?)(?:\s+project)?\s+in\s+(\S+)\s+using\s+(.+?)\s*$",
    )
    .ok()?;
    let cap = re.captures(input.trim())?;
    let project = cap.get(1)?.as_str().trim().trim_start_matches("the ");
    let path = cap.get(2)?.as_str().trim();
    let lang = cap.get(3)?.as_str().trim().trim_end_matches('.');
    if project.is_empty() || path.is_empty() || lang.is_empty() {
        return None;
    }
    Some(format!(
        "The {} project is stored at {} and uses {}.",
        project, path, lang
    ))
}

fn extract_workspace_path(input: &str) -> Option<String> {
    let re = Regex::new(r"(?i)in\s+(\S+)\s+using").ok()?;
    let cap = re.captures(input)?;
    Some(cap.get(1)?.as_str().to_string())
}

/// "always X", "prefer X", "I prefer X", "we prefer", "use X for Y"
fn match_preference(input: &str) -> Option<String> {
    let lower = input.to_ascii_lowercase();

    // Explicit test preference
    if lower.contains("cargo test before")
        || lower.contains("run tests before")
        || lower.contains("test before saying done")
        || lower.contains("test before marking")
    {
        return Some("User prefers running tests before marking task done.".to_string());
    }

    let re =
        Regex::new(r"(?i)\b(?:always\s+\w|prefer(?:s|red)?\s+\w|i prefer|we prefer|use .+ for)\b")
            .ok()?;
    if re.is_match(input) && input.split_whitespace().count() >= 4 {
        return Some(crate::conductor::memory_utils::truncate(input, 150));
    }
    None
}

/// "never X", "don't do X", "avoid X", "do NOT X", "broken if"
fn match_warning(input: &str) -> Option<String> {
    let re =
        Regex::new(r"(?i)\b(?:never\s+\w+|don't\s+\w+|do\s+not\s+\w+|avoid\s+\w+|broken\s+if)\b")
            .ok()?;
    if re.is_match(input) && input.split_whitespace().count() >= 4 {
        return Some(crate::conductor::memory_utils::truncate(input, 150));
    }
    None
}

/// "decided to X", "going with X", "switched to X", "use X instead of Y", "replacing X with Y"
fn match_decision(input: &str) -> Option<String> {
    let re = Regex::new(
        r"(?i)\b(?:decided\s+to|going\s+with|switched\s+to|migrating\s+to|use\s+\w+\s+instead|replacing\s+\w+\s+with)\b",
    )
    .ok()?;
    if re.is_match(input) {
        return Some(crate::conductor::memory_utils::truncate(input, 150));
    }
    None
}

/// error[E0XXX], panicked at, FAILED, Error:, fatal, exception
fn match_error(input: &str) -> Option<String> {
    let re = Regex::new(
        r"(?i)\b(?:error\[|panicked\s+at|thread\s+'[^']*'\s+panicked|FAILED:|Error:|fatal:|exception:)\b",
    )
    .ok()?;
    if re.is_match(input) {
        return Some(crate::conductor::memory_utils::truncate(input, 200));
    }
    None
}

/// "fixed X", "resolved X", "works now", "root cause", "the issue was"
fn match_fix(input: &str) -> Option<String> {
    let re = Regex::new(
        r"(?i)\b(?:fixed|resolved|works\s+now|root\s+cause|the\s+issue\s+was|the\s+bug\s+was|the\s+cause\s+was)\b",
    )
    .ok()?;
    if re.is_match(input) {
        return Some(crate::conductor::memory_utils::truncate(input, 200));
    }
    None
}

// ── Entity extractors ─────────────────────────────────────────────────────────

fn extract_file_entities(text: &str) -> Vec<String> {
    let re = match Regex::new(r"(?:^|[\s(\[])([~.]?/(?:[\w.\-]+/)*[\w.\-]+\.\w{1,10})") {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    re.captures_iter(text)
        .filter_map(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .take(5)
        .collect()
}

fn extract_function_entities(text: &str) -> Vec<String> {
    let re = match Regex::new(r"\b(?:fn|def|func|function)\s+(\w+)") {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    re.captures_iter(text)
        .filter_map(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .take(5)
        .collect()
}

fn extract_error_codes(text: &str) -> Vec<String> {
    let re = match Regex::new(r"error\[([A-Z]\d+)\]") {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    re.captures_iter(text)
        .filter_map(|c| c.get(1))
        .map(|m| format!("error[{}]", m.as_str()))
        .collect()
}

/// Match crate paths like `tokio::spawn`, `sqlx::query`
fn extract_crate_paths(text: &str) -> Vec<String> {
    let re = match Regex::new(r"\b([a-z][a-z0-9_]{1,20})::[a-z_]") {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    re.captures_iter(text)
        .filter_map(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .take(4)
        .collect()
}

/// Git SHAs and branch names
fn extract_git_refs(text: &str) -> Vec<String> {
    let mut refs = Vec::new();
    // Commit SHA (7+ hex chars)
    if let Ok(re) = Regex::new(r"\b([a-f0-9]{7,40})\b") {
        refs.extend(
            re.captures_iter(text)
                .filter_map(|c| c.get(1))
                .map(|m| format!("commit:{}", m.as_str()))
                .take(2),
        );
    }
    // Branch name
    if let Ok(re) = Regex::new(r"(?i)\b(?:branch|checkout|merge)\s+(\S+)") {
        refs.extend(
            re.captures_iter(text)
                .filter_map(|c| c.get(1))
                .map(|m| format!("branch:{}", m.as_str()))
                .take(2),
        );
    }
    refs
}

// ── Domain vocabulary → topics ────────────────────────────────────────────────

/// Keyword clusters that map to topic labels.
/// Match if ≥2 keywords from a cluster appear in the text.
const DOMAIN_VOCAB: &[(&str, &str)] = &[
    (
        "git commit branch rebase merge pr pull push diff stash",
        "git",
    ),
    (
        "cargo crate rust tokio async trait impl struct enum lifetim",
        "rust",
    ),
    (
        "SELECT INSERT UPDATE DELETE FROM WHERE JOIN TABLE INDEX migration",
        "sql",
    ),
    (
        "docker kubernetes helm kubectl pod deployment service ingress",
        "devops",
    ),
    (
        "python pip pytest pandas numpy torch fastapi pydantic",
        "python",
    ),
    (
        "typescript javascript react node npm yarn webpack vite",
        "frontend",
    ),
    (
        "security auth token jwt oauth permission role secret credential",
        "security",
    ),
    (
        "test unit integration mock fixture assert coverage",
        "testing",
    ),
    (
        "error panic bug crash fix issue resolve exception traceback",
        "debugging",
    ),
    (
        "deploy release build artifact ci cd pipeline workflow action",
        "deployment",
    ),
    (
        "llm model embedding vector inference training fine-tune",
        "ml",
    ),
    (
        "memory heap stack leak allocat oom overflow buffer",
        "memory",
    ),
];

fn topics_from_vocabulary(text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    DOMAIN_VOCAB
        .iter()
        .filter_map(|(keywords, topic)| {
            let hits = keywords
                .split_whitespace()
                .filter(|kw| lower.contains(kw))
                .count();
            if hits >= 2 {
                Some(topic.to_string())
            } else {
                None
            }
        })
        .collect()
}

// ── Importance boosting ───────────────────────────────────────────────────────

fn boost_importance(text: &str, mut base: f64) -> f64 {
    let lower = text.to_ascii_lowercase();

    if lower.contains("important") || lower.contains("critical") {
        base += 0.15;
    }
    if lower.contains("never") || lower.contains("avoid") || lower.contains("broken") {
        base += 0.12;
    }
    if lower.contains("architecture")
        || lower.contains("design decision")
        || lower.contains("decided")
    {
        base += 0.10;
    }
    if lower.contains("error") || lower.contains("panic") || lower.contains("failed") {
        base += 0.10;
    }
    if lower.contains("fixed") || lower.contains("resolved") || lower.contains("root cause") {
        base += 0.08;
    }
    if lower.contains("security") || lower.contains("vulnerability") || lower.contains("cve") {
        base += 0.15;
    }
    // Penalise very short / trivial queries
    if text.split_whitespace().count() < 4 {
        base -= 0.15;
    }

    base
}

fn push_topic(topics: &mut Vec<String>, topic: &str) {
    if !topics.iter().any(|t| t == topic) {
        topics.push(topic.to_string());
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// LLM extraction path
// ─────────────────────────────────────────────────────────────────────────────

async fn extract_via_llm(
    router: &LLMRouter,
    input: &str,
    result: &str,
) -> Result<ExtractionOutput> {
    use crate::conductor::memory_prompts::INGEST_PROMPT;
    use crate::conductor::memory_utils::strip_markdown_fences;
    use crate::conductor::types::IngestExtraction;
    use crate::llm::Message;
    use std::time::Duration;
    use tokio::time::timeout;

    let content = format!("{}INPUT:\n{}\n\nRESULT:\n{}", INGEST_PROMPT, input, result);
    let messages = vec![
        Message::system(
            "You are a structured data extraction system. \
             Respond with ONLY valid JSON, no markdown fences, no explanation.",
        ),
        Message::user(&content),
    ];

    let (response, _provider) = timeout(Duration::from_secs(45), router.call(&messages))
        .await
        .map_err(|_| anyhow::anyhow!("LLM extraction timed out after 45s"))?
        .map_err(|e| anyhow::anyhow!("LLM call failed: {}", e))?;

    let text = match response {
        crate::llm::LLMResponse::FinalAnswer(a) => a.content,
        crate::llm::LLMResponse::ToolCall(tc) => tc.arguments,
    };

    let extraction: IngestExtraction = serde_json::from_str(&strip_markdown_fences(&text))
        .map_err(|e| anyhow::anyhow!("Failed to parse LLM extraction: {}", e))?;

    if extraction.summary.is_empty() {
        return Err(anyhow::anyhow!("LLM returned empty summary"));
    }

    // MemoryKind and facts are always set via heuristics regardless of LLM path,
    // because the LLM prompt doesn't output them — keeping the prompt minimal.
    let heuristic = extract_heuristic(input);

    Ok(ExtractionOutput {
        summary: extraction.summary,
        entities: extraction.entities.into_iter().take(10).collect(),
        topics: extraction.topics.into_iter().take(5).collect(),
        importance: extraction.importance.clamp(0.0, 1.0),
        kind: heuristic.kind,
        facts: heuristic.facts,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Factory — builds the right extractor from config
// ─────────────────────────────────────────────────────────────────────────────

use crate::conductor::types::ExtractionBackend;

/// Build the right `MemoryExtractor` implementation from config.
pub fn build_extractor(
    backend: &ExtractionBackend,
    router: Arc<LLMRouter>,
) -> Arc<dyn MemoryExtractor> {
    match backend {
        ExtractionBackend::Heuristic => Arc::new(HeuristicExtractor::new()),
        ExtractionBackend::Local | ExtractionBackend::Cloud => {
            // Both local and cloud route through the LLM router — the router's
            // own provider ranking handles local-only / cloud-only preference.
            Arc::new(LlmExtractor::new(router))
        }
        ExtractionBackend::Auto => Arc::new(AutoExtractor::new(router)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heuristic_remembered_fact() {
        let out = extract_heuristic("remember that the deploy command is cargo build --release");
        assert_eq!(out.kind, MemoryKind::Fact);
        assert!(out.importance >= 0.85);
        assert!(out.summary.contains("cargo build --release"));
        assert!(!out.facts.is_empty());
    }

    #[test]
    fn test_heuristic_remembered_fact_colon_prefix() {
        let out = extract_heuristic("remember: the rove project uses Rust");
        assert_eq!(out.kind, MemoryKind::Fact);
        assert!(out.importance >= 0.85);
        assert!(out.summary.contains("rove project uses Rust"));
    }

    #[test]
    fn test_heuristic_user_property() {
        let out = extract_heuristic("my preferred linter is clippy");
        assert_eq!(out.kind, MemoryKind::Fact);
        assert!(out.importance >= 0.9);
        assert!(out
            .facts
            .iter()
            .any(|(k, v)| k == "preferred linter" && v == "clippy"));
    }

    #[test]
    fn test_heuristic_preference() {
        let out = extract_heuristic("always run cargo test before saying done");
        assert_eq!(out.kind, MemoryKind::Preference);
        assert!(out.importance >= 0.85);
    }

    #[test]
    fn test_heuristic_warning() {
        let out = extract_heuristic("never use unwrap() in production code paths");
        assert_eq!(out.kind, MemoryKind::Warning);
        assert!(out.importance >= 0.88);
    }

    #[test]
    fn test_heuristic_decision() {
        let out = extract_heuristic("decided to use sqlx instead of diesel for the new service");
        assert_eq!(out.kind, MemoryKind::Decision);
        assert!(out.importance >= 0.80);
    }

    #[test]
    fn test_heuristic_error() {
        let out = extract_heuristic("error[E0499]: cannot borrow `data` as mutable more than once");
        assert_eq!(out.kind, MemoryKind::Error);
        assert!(out.entities.iter().any(|e| e.contains("E0499")));
    }

    #[test]
    fn test_heuristic_fix() {
        let out = extract_heuristic(
            "fixed the deadlock — root cause was holding a lock across await points",
        );
        assert_eq!(out.kind, MemoryKind::Fix);
    }

    #[test]
    fn test_heuristic_combined_reads_result_text() {
        let out = extract_heuristic_combined(
            "investigate the failure",
            "fixed the deadlock by dropping the lock before await",
        );
        assert_eq!(out.kind, MemoryKind::Fix);
        assert!(out.summary.to_ascii_lowercase().contains("fixed"));
    }

    #[test]
    fn test_heuristic_entities_file_and_crate() {
        let out = extract_heuristic("tokio::spawn panicked in /src/engine/memory.rs");
        assert!(out.entities.iter().any(|e| e.contains("memory.rs")));
        assert!(out.entities.iter().any(|e| e == "tokio"));
    }

    #[test]
    fn test_heuristic_topics_rust() {
        let out = extract_heuristic("cargo build failed because the struct lifetime is wrong");
        assert!(out.topics.iter().any(|t| t == "rust"));
    }

    #[test]
    fn test_heuristic_workspace() {
        let out = extract_heuristic("I work on the rove project in ~/workspace/rove using Rust");
        assert_eq!(out.kind, MemoryKind::Fact);
        assert!(out.facts.iter().any(|(k, _)| k == "workspace_path"));
    }

    #[test]
    fn test_short_input_low_importance() {
        let out = extract_heuristic("yes");
        assert!(out.importance < 0.4);
    }
}
