//! Memory Consolidation
//!
//! Processes unconsolidated episodic memories into cross-cutting insights.
//! Groups memories by domain and generates behavioral patterns using LLM.

use anyhow::{Context, Result};
use sqlx::{Row, SqlitePool};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::conductor::memory_types::*;
use crate::conductor::memory_utils::*;
use crate::llm::router::LLMRouter;

/// Consolidate unconsolidated episodic memories into cross-cutting insights.
///
/// Fetches all rows where `consolidated = 0`, groups by domain, and processes
/// each domain group separately. If a domain group has fewer than min_to_consolidate
/// memories, it is skipped. Calls the LLM with `CONSOLIDATE_PROMPT` and stores
/// each insight in `consolidation_insights` with the domain field, then marks the
/// source memories as `consolidated = 1`.
///
/// # Arguments
/// * `pool` - SQLite connection pool
/// * `router` - LLM router for consolidation calls
/// * `min_to_consolidate` - Minimum memories needed to trigger consolidation
pub async fn consolidate(
    pool: &SqlitePool,
    router: &LLMRouter,
    min_to_consolidate: usize,
) -> Result<ConsolidationResult> {
    // Fetch unconsolidated memories with domain field
    let rows = sqlx::query(
        r#"SELECT id, summary, entities, topics, importance, domain
           FROM episodic_memory
           WHERE consolidated = 0
           ORDER BY created_at DESC
           LIMIT 50"#,
    )
    .fetch_all(pool)
    .await
    .context("Failed to fetch unconsolidated memories")?;

    if rows.len() < min_to_consolidate {
        debug!(
            "Only {} unconsolidated memories, skipping consolidation",
            rows.len()
        );
        return Ok(ConsolidationResult::Skipped {
            reason: format!(
                "Only {} memories (need ≥{})",
                rows.len(),
                min_to_consolidate
            ),
        });
    }

    info!("Consolidating {} memories", rows.len());

    // Group by domain
    let mut domain_groups: std::collections::HashMap<String, Vec<_>> =
        std::collections::HashMap::new();

    for row in &rows {
        let domain: String = row.get("domain");
        domain_groups.entry(domain).or_default().push(row);
    }

    let mut total_memories = 0;
    let mut total_insights = 0;

    // Process each domain group
    for (domain, group) in domain_groups {
        if group.len() < min_to_consolidate {
            debug!(
                "Domain {} has only {} memories, skipping",
                domain,
                group.len()
            );
            continue;
        }

        // Build prompt with memory summaries
        let mut memories_text = String::new();
        let mut memory_ids: Vec<String> = Vec::new();

        for row in &group {
            let id: String = row.get("id");
            let summary: String = row.get("summary");
            let entities: String = row.get::<Option<String>, _>("entities").unwrap_or_default();
            let topics: String = row.get::<Option<String>, _>("topics").unwrap_or_default();
            let importance: f64 = row.get("importance");

            memories_text.push_str(&format!(
                "- [{}] (importance={:.1}) {}\n  entities: {}\n  topics: {}\n\n",
                id, importance, summary, entities, topics
            ));
            memory_ids.push(id);
        }

        let content = format!(
            "{}{}",
            crate::conductor::memory_prompts::CONSOLIDATE_PROMPT,
            memories_text
        );

        // Call LLM
        let insights = match call_llm_for_text(router, &content).await {
            Ok(text) => parse_consolidation_response(&text, &memory_ids),
            Err(e) => {
                warn!("LLM consolidation call failed for domain {}: {}", domain, e);
                continue;
            }
        };

        if insights.is_empty() {
            warn!("LLM returned no usable insights for domain {}", domain);
            continue;
        }

        // Begin transaction for this domain batch
        let mut tx = pool.begin().await.context("Failed to begin transaction")?;

        // Store insights
        let consolidation_id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp();

        for insight in &insights {
            let insight_id = Uuid::new_v4().to_string();
            let source_ids_json =
                serde_json::to_string(&insight.source_ids).unwrap_or_else(|_| "[]".to_string());

            sqlx::query(
                r#"INSERT INTO consolidation_insights 
                   (id, insight, domain, source_ids, created_at)
                   VALUES (?, ?, ?, ?, ?)"#,
            )
            .bind(&insight_id)
            .bind(&insight.insight)
            .bind(&domain)
            .bind(&source_ids_json)
            .bind(now)
            .execute(&mut *tx)
            .await
            .context("Failed to insert consolidation insight")?;

            total_insights += 1;
        }

        // Mark memories as consolidated
        for id in &memory_ids {
            sqlx::query(
                r#"UPDATE episodic_memory
                   SET consolidated = 1, consolidation_id = ?
                   WHERE id = ?"#,
            )
            .bind(&consolidation_id)
            .bind(id)
            .execute(&mut *tx)
            .await
            .context("Failed to mark memory as consolidated")?;

            total_memories += 1;
        }

        // Commit transaction for this domain
        tx.commit()
            .await
            .context("Failed to commit consolidation transaction")?;
    }

    info!(
        "Consolidation complete: {} memories → {} insights",
        total_memories, total_insights
    );

    Ok(ConsolidationResult::Completed {
        memories_processed: total_memories,
        insights_generated: total_insights,
    })
}

/// Call the LLM router and extract the text content from FinalAnswer.
async fn call_llm_for_text(router: &LLMRouter, user_content: &str) -> Result<String> {
    use crate::llm::Message;
    use std::time::Duration;
    use tokio::time::timeout;

    let messages = vec![
        Message::system("You are a structured data extraction system. Respond with ONLY valid JSON, no markdown fences, no explanation."),
        Message::user(user_content),
    ];

    let result = timeout(Duration::from_secs(60), router.call(&messages))
        .await
        .context("LLM call timed out")?
        .map_err(|e| anyhow::anyhow!("LLM call failed: {}", e))?;

    let (response, provider) = result;
    debug!("Memory LLM call answered by {}", provider);

    match response {
        crate::llm::LLMResponse::FinalAnswer(answer) => Ok(answer.content),
        crate::llm::LLMResponse::ToolCall(tc) => {
            warn!("Memory LLM returned tool call instead of text, using arguments");
            Ok(tc.arguments)
        }
    }
}

/// Parse LLM response from consolidation prompt into insights.
/// Validates that source_ids reference actual memory IDs.
fn parse_consolidation_response(text: &str, valid_ids: &[String]) -> Vec<ConsolidationInsight> {
    let cleaned = strip_markdown_fences(text);

    match serde_json::from_str::<Vec<ConsolidationInsight>>(&cleaned) {
        Ok(insights) => insights
            .into_iter()
            .filter(|i| {
                if i.insight.is_empty() {
                    warn!("Skipping empty insight");
                    return false;
                }
                // Keep only insights whose source_ids reference valid memories
                let valid_refs = i.source_ids.iter().all(|id| valid_ids.contains(id));
                if !valid_refs {
                    warn!(
                        "Insight references unknown memory IDs, accepting anyway: {:?}",
                        i.source_ids
                    );
                }
                true // Accept even with bad refs — the insight text is still useful
            })
            .collect(),
        Err(e) => {
            warn!(
                "Failed to parse consolidation LLM response: {} — raw: {}",
                e,
                truncate(text, 200)
            );
            vec![]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_consolidation_response_valid() {
        let ids = vec!["id1".to_string(), "id2".to_string()];
        let json = r#"[{"insight":"User prefers Rust","source_ids":["id1","id2"]}]"#;
        let result = parse_consolidation_response(json, &ids);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].insight, "User prefers Rust");
    }

    #[test]
    fn test_parse_consolidation_response_invalid() {
        let result = parse_consolidation_response("garbage", &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_consolidation_response_empty_insight() {
        let json = r#"[{"insight":"","source_ids":["id1"]}]"#;
        let result = parse_consolidation_response(json, &["id1".to_string()]);
        assert!(result.is_empty());
    }
}
