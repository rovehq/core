//! Memory system prompts
//!
//! Contains the LLM prompts used for memory operations.

/// Prompt for memory ingestion - extracts structured data from task results
pub const INGEST_PROMPT: &str = r#"You are a memory extraction system. Given a task input and its result, extract structured memory data.

Respond with ONLY a JSON object (no markdown, no explanation):
{
  "summary": "One-sentence summary of what happened",
  "entities": ["list", "of", "key", "entities", "mentioned"],
  "topics": ["list", "of", "topics", "or", "domains"],
  "importance": 0.7
}

Rules:
- summary: One clear sentence describing the task and outcome
- entities: Proper nouns, tool names, file paths, technologies mentioned (max 10)
- topics: Abstract domains/categories (max 5). Examples: "rust", "security", "database", "deployment"
- importance: Float 0.0-1.0. Higher for: errors fixed, architecture decisions, security changes, user corrections. Lower for: routine reads, simple queries

Task input and result:
"#;

/// Prompt for memory consolidation - finds patterns across memories
pub const CONSOLIDATE_PROMPT: &str = r#"You are a memory consolidation system. Given a set of episodic memories, find cross-cutting patterns and generate insights.

Respond with ONLY a JSON array of insight objects (no markdown, no explanation):
[
  {
    "insight": "Clear statement of the pattern or connection found",
    "source_ids": ["id1", "id2", "id3"]
  }
]

Rules:
- Each insight should connect 2+ memories
- Focus on: recurring patterns, user preferences, common error types, workflow habits
- Be specific and actionable, not vague
- Generate 1-3 insights maximum

Memories to analyze:
"#;
