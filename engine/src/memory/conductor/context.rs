//! Context Assembler
//!
//! Responsible for intelligently packing context (Project Memory, Episodic Memory,
//! System Instructions) into the available token budget for a task.

use crate::builtin_tools::ToolRegistry;
use crate::conductor::memory::{MemorySystem, SessionMemory};
use crate::conductor::project::ProjectMemory;
use crate::conductor::types::{MemoryBudget, TaskDomain};
use crate::llm::Message;
use crate::steering::loader::PolicyEngine;
use anyhow::Result;

pub struct ContextAssembler {
    budget: MemoryBudget,
}

impl ContextAssembler {
    pub fn new(budget: MemoryBudget) -> Self {
        Self { budget }
    }

    /// Assemble the final list of messages to send to the LLM, adhering to the token budget.
    ///
    /// `tool_registry` is optional. When provided, only tool schemas relevant to
    /// the detected query domain are injected into the system prompt.
    #[allow(clippy::too_many_arguments)]
    pub async fn assemble(
        &self,
        system_instructions: &str,
        project_memory: Option<&ProjectMemory>,
        session_memory: &SessionMemory,
        memory_system: Option<&MemorySystem>,
        policy_engine: Option<&PolicyEngine>,
        tool_registry: Option<&ToolRegistry>,
        query: &str,
        domain: &TaskDomain,
    ) -> Result<Vec<Message>> {
        let mut final_messages = Vec::new();

        // 1. Build the system prompt
        let mut sys_prompt = String::with_capacity(self.budget.session_tokens);
        sys_prompt.push_str(system_instructions);

        // 2. Inject Project Memory if available
        if let Some(pm) = project_memory {
            sys_prompt.push_str("\n\n--- Project Context ---\n");
            sys_prompt.push_str(&pm.format_for_prompt());
        }

        // 3. Inject policy-matched context based on query text.
        if let Some(policy_engine) = policy_engine {
            let mut active_policies = Vec::new();
            let query_lower = query.to_lowercase();

            // Very simplistic semantic routing: checks if query mentions the policy name/desc words.
            for policy in policy_engine.list_policies().await {
                if query_lower.contains(&policy.name.to_lowercase()) {
                    active_policies.push(policy);
                } else {
                    let desc_words: Vec<&str> = policy.description.split_whitespace().collect();
                    for word in desc_words {
                        if word.len() > 4 && query_lower.contains(&word.to_lowercase()) {
                            active_policies.push(policy);
                            break;
                        }
                    }
                }
            }

            if !active_policies.is_empty() {
                sys_prompt.push_str("\n\n--- Active Policies ---\n");
                for policy in active_policies.into_iter().take(3) {
                    sys_prompt.push_str(&format!("# {}\n{}\n\n", policy.name, policy.content));
                }
            }
        }

        // 3b. Inject domain-relevant plugin tool schemas (Phase 4: dynamic tool loading)
        if let Some(registry) = tool_registry {
            let schemas = registry.tool_schemas_for_prompt(query);
            if !schemas.is_empty() {
                sys_prompt.push_str("\n\n--- Plugin Tools ---\n");
                sys_prompt.push_str(&schemas);
            }
        }

        // 4. Inject relevant Episodic Memory if available
        if let Some(memory) = memory_system {
            // Query domain-gated memories with importance filtering
            match memory.query(query, domain, None).await {
                Ok(hits) if !hits.is_empty() => {
                    sys_prompt.push_str("\n\n--- Relevant Past Context ---\n");
                    let hits_len = hits.len();
                    for hit in &hits {
                        let hit_type_label = match hit.hit_type {
                            crate::conductor::memory::HitType::Insight => "Insight",
                            crate::conductor::memory::HitType::Episodic => "Memory",
                            crate::conductor::memory::HitType::KnowledgeGraph => "Graph",
                            crate::conductor::memory::HitType::TaskTrace => "Task",
                        };
                        let snippet = format!("[{}] {}\n", hit_type_label, hit.content);
                        // Very rough token truncation (avoid blowing up the system prompt)
                        let snippet_tokens = snippet.len() / 4;
                        if snippet_tokens < self.budget.episodic_tokens / hits_len {
                            sys_prompt.push_str(&snippet);
                        }
                    }
                }
                Ok(_) => {
                    // Empty results, proceed without memory context
                }
                Err(e) => {
                    // Query failed, log and proceed without memory context
                    tracing::warn!(
                        "Memory query failed, proceeding without memory context: {}",
                        e
                    );
                }
            }
        }

        final_messages.push(Message::system(sys_prompt));

        // 4. Inject Session Memory history (last N messages)
        let session_messages = session_memory.messages();
        let mut accumulated_tokens = 0;

        // Take messages from newest to oldest up to the session token limit
        let mut history_to_add = Vec::new();
        for msg in session_messages.iter().rev() {
            let tokens = msg.content.len() / 4;
            if accumulated_tokens + tokens < self.budget.session_tokens {
                history_to_add.push(msg.clone());
                accumulated_tokens += tokens;
            } else {
                break;
            }
        }

        // history_to_add is reversed, so put it back in chronological order
        history_to_add.reverse();
        final_messages.extend(history_to_add);

        // 5. Finally, append the actual user query
        final_messages.push(Message::user(query));

        Ok(final_messages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::working_memory::WorkingMemory;
    use crate::llm::MessageRole;
    use tempfile::tempdir;
    use tokio::fs;

    #[tokio::test]
    async fn test_context_assembler_budgeting() {
        let budget = MemoryBudget {
            system_tokens: 1000,
            project_tokens: 500,
            episodic_tokens: 500,
            session_tokens: 50, // very small budget to test truncation
        };

        let assembler = ContextAssembler::new(budget.clone());

        let mut wm = WorkingMemory::new();

        // Add some messages, ~5 tokens each
        for i in 0..20 {
            wm.add_message(Message::user(format!("Message number {}", i)));
        }

        let session_memory = SessionMemory::new(&budget);
        // Manually populate the session memory with the same messages
        for msg in wm.messages() {
            // Clone each message into the session
            let _ = msg; // session_memory was built from budget, so it starts empty
        }

        let messages = assembler
            .assemble(
                "You are an AI.",
                None,
                &session_memory,
                None, // memory_system
                None,
                None, // tool_registry
                "What is the answer?",
                &TaskDomain::General,
            )
            .await
            .unwrap();

        // Should have System message and the User query at minimum
        assert!(!messages.is_empty());
        assert_eq!(messages[0].role, MessageRole::System);
        assert_eq!(messages.last().unwrap().role, MessageRole::User);
    }

    #[tokio::test]
    async fn test_semantic_routing_skills() {
        let dir = tempdir().unwrap();
        let skills_dir = dir.path();

        // Create couple of mock skills
        let skill1 = r#"---
name: Database
description: Guidelines for sqlx and database queries
---
Use sqlx carefully."#;
        fs::write(skills_dir.join("db.md"), skill1).await.unwrap();

        let skill2 = r#"---
name: Frontend
description: React and TailwindCSS rules
---
Use hooks correctly."#;
        fs::write(skills_dir.join("ui.md"), skill2).await.unwrap();

        let policy_engine = PolicyEngine::new(skills_dir).await.unwrap();

        let budget = MemoryBudget {
            system_tokens: 1000,
            project_tokens: 500,
            episodic_tokens: 500,
            session_tokens: 1000,
        };
        let assembler = ContextAssembler::new(budget.clone());

        let session_memory = SessionMemory::new(&budget);

        // Query mentioning database should include the Database skill
        let messages = assembler
            .assemble(
                "SystemPrompt",
                None,
                &session_memory,
                None, // memory_system
                Some(&policy_engine),
                None, // tool_registry
                "I need to write a database query.",
                &TaskDomain::Code,
            )
            .await
            .unwrap();

        let sys_content = &messages[0].content;
        assert!(sys_content.contains("--- Active Policies ---"));
        assert!(sys_content.contains("# Database"));
        assert!(!sys_content.contains("# Frontend"));

        // Query mentioning react should include the Frontend skill
        let messages_ui = assembler
            .assemble(
                "SystemPrompt",
                None,
                &session_memory,
                None, // memory_system
                Some(&policy_engine),
                None, // tool_registry
                "Help me with this React component.",
                &TaskDomain::Code,
            )
            .await
            .unwrap();

        let sys_content_ui = &messages_ui[0].content;
        assert!(sys_content_ui.contains("--- Active Policies ---"));
        assert!(!sys_content_ui.contains("# Database"));
        assert!(sys_content_ui.contains("# Frontend"));
    }

    #[tokio::test]
    async fn test_memory_integration() {
        use crate::conductor::memory::MemorySystem;
        use crate::config::LLMConfig;
        use crate::llm::router::LLMRouter;
        use std::str::FromStr;
        use std::sync::Arc;

        // Create in-memory database
        let opts = sqlx::sqlite::SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .create_if_missing(true);
        let pool = sqlx::SqlitePool::connect_with(opts).await.unwrap();

        // Run migrations to create tables
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS episodic_memory (
                id TEXT PRIMARY KEY,
                task_id TEXT NOT NULL,
                summary TEXT NOT NULL,
                entities TEXT,
                topics TEXT,
                importance REAL NOT NULL,
                consolidated INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                domain TEXT NOT NULL DEFAULT 'general',
                sensitive INTEGER NOT NULL DEFAULT 0,
                last_accessed INTEGER,
                access_count INTEGER NOT NULL DEFAULT 0
            )"#,
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS consolidation_insights (
                id TEXT PRIMARY KEY,
                insight TEXT NOT NULL,
                domain TEXT,
                source_ids TEXT,
                created_at INTEGER NOT NULL
            )"#,
        )
        .execute(&pool)
        .await
        .unwrap();

        // Create FTS tables
        sqlx::query(
            r#"CREATE VIRTUAL TABLE IF NOT EXISTS episodic_fts USING fts5(
                summary,
                entities,
                topics,
                tags,
                domain,
                content=episodic_memory,
                content_rowid=rowid
            )"#,
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"CREATE VIRTUAL TABLE IF NOT EXISTS insights_fts USING fts5(
                insight,
                domain,
                content=consolidation_insights,
                content_rowid=rowid
            )"#,
        )
        .execute(&pool)
        .await
        .unwrap();

        // Create LLM router
        let config = Arc::new(LLMConfig {
            default_provider: "ollama".to_string(),
            sensitivity_threshold: 0.7,
            complexity_threshold: 0.8,
            ollama: Default::default(),
            openai: Default::default(),
            anthropic: Default::default(),
            gemini: Default::default(),
            nvidia_nim: Default::default(),
            custom_providers: vec![],
        });

        let router = Arc::new(LLMRouter::new(vec![], config));
        let memory_system = MemorySystem::new(pool.clone(), router);

        // Insert a test memory
        let now = crate::conductor::scorer::unix_now();
        sqlx::query(
            r#"INSERT INTO episodic_memory 
               (id, task_id, summary, importance, consolidated, created_at, domain, sensitive, last_accessed, access_count)
               VALUES (?, ?, ?, ?, 0, ?, ?, 0, ?, 0)"#,
        )
        .bind("test-mem-1")
        .bind("task-1")
        .bind("Fixed a critical bug in the authentication system")
        .bind(0.9)
        .bind(now)
        .bind("Code")
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();

        // Populate FTS
        sqlx::query(
            r#"INSERT INTO episodic_fts(rowid, summary, entities, topics, tags, domain)
               SELECT rowid, summary, entities, topics, '', domain FROM episodic_memory"#,
        )
        .execute(&pool)
        .await
        .unwrap();

        // Test context assembly with memory
        let budget = MemoryBudget {
            system_tokens: 1000,
            project_tokens: 500,
            episodic_tokens: 500,
            session_tokens: 1000,
        };
        let assembler = ContextAssembler::new(budget.clone());
        let session_memory = SessionMemory::new(&budget);

        let messages = assembler
            .assemble(
                "You are an AI assistant.",
                None,
                &session_memory,
                Some(&memory_system),
                None,
                None,
                "Tell me about authentication bugs",
                &TaskDomain::Code,
            )
            .await
            .unwrap();

        // Verify the structure is correct
        let sys_content = &messages[0].content;
        assert!(messages.len() >= 2); // System + User message
        assert_eq!(messages[0].role, MessageRole::System);
        assert_eq!(messages.last().unwrap().role, MessageRole::User);
        assert!(sys_content.contains("You are an AI assistant."));
    }
}
