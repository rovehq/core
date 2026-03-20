use anyhow::Result;
use std::path::PathBuf;
use tracing::debug;

use crate::gateway::Task;
use crate::llm::Message;
use crate::risk_assessor::RiskTier;
use crate::steering::types::MergedDirectives;
use sdk::{Complexity, Route, TaskDomain};

use super::AgentCore;

pub(super) struct TaskContext {
    pub(super) domain_str: String,
    pub(super) domain: TaskDomain,
    pub(super) complexity: Complexity,
    pub(super) route: Route,
    pub(super) sensitive: bool,
}

impl AgentCore {
    pub(super) async fn initialize_task_context(
        &mut self,
        task: &Task,
        risk_tier: RiskTier,
    ) -> Result<TaskContext> {
        self.memory.clear();
        self.steering_preflight_commands.clear();
        self.steering_after_write_commands.clear();
        self.steering_executed_commands.clear();
        let mut system_prompt = self.tools.system_prompt_for_query(&task.input);
        let dispatch_result = self.dispatch_brain.classify(&task.input);
        let domain_name = dispatch_result.domain_label.to_lowercase();
        self.current_task_sensitive = dispatch_result.sensitive;

        self.apply_steering(
            &task.input,
            risk_tier,
            Some(&domain_name),
            &mut system_prompt,
        )
        .await;
        let domain_str = dispatch_result.domain.to_string();

        debug!(
            task_id = %task.id,
            domain = ?dispatch_result.domain,
            domain_label = %dispatch_result.domain_label,
            domain_confidence = dispatch_result.domain_confidence,
            complexity = ?dispatch_result.complexity,
            sensitive = dispatch_result.sensitive,
            injection_score = dispatch_result.injection_score,
            route = ?dispatch_result.route,
            "Dispatch brain classification complete"
        );

        self.inject_memory_context(&task.input, dispatch_result.domain, &mut system_prompt)
            .await;
        self.inject_preferences(&mut system_prompt).await;

        self.memory.add_message(Message::system(system_prompt));
        self.memory.add_message(Message::user(&task.input));

        Ok(TaskContext {
            domain_str,
            domain: dispatch_result.domain,
            complexity: dispatch_result.complexity,
            route: dispatch_result.route,
            sensitive: dispatch_result.sensitive,
        })
    }

    async fn apply_steering(
        &mut self,
        task_input: &str,
        risk_tier: RiskTier,
        domain: Option<&str>,
        system_prompt: &mut String,
    ) {
        let Some(steering) = self.steering.as_mut() else {
            return;
        };

        let risk_tier_u8 = match risk_tier {
            RiskTier::Tier0 => 0,
            RiskTier::Tier1 => 1,
            RiskTier::Tier2 => 2,
        };
        steering
            .auto_activate(task_input, risk_tier_u8, domain)
            .await;
        let directives = steering.get_directives_for_task(task_input).await;
        let active_skills = steering.active_skills().await;
        let matched_hints = steering.matched_hints(task_input).await;
        let _ = steering;

        self.configure_steering_commands(task_input, &directives, &matched_hints);
        if !directives.system_prefix.is_empty() {
            *system_prompt = format!("{}\n\n{}", directives.system_prefix, system_prompt);
        }
        if !directives.system_suffix.is_empty() {
            *system_prompt = format!("{}\n\n{}", system_prompt, directives.system_suffix);
        }

        debug!("Active steering directives: {:?}", active_skills);
    }

    fn configure_steering_commands(
        &mut self,
        task_input: &str,
        directives: &MergedDirectives,
        matched_hints: &[String],
    ) {
        let task_input_lower = task_input.to_ascii_lowercase();
        let steering_text = format!(
            "{}\n{}\n{}",
            directives.system_prefix,
            directives.system_suffix,
            matched_hints.join("\n")
        )
        .to_ascii_lowercase();

        if task_input_lower.contains("commit") && steering_text.contains("git diff --stat") {
            self.steering_preflight_commands
                .push("git diff --stat".to_string());
        }

        if task_input_lower.contains("refactor") && steering_text.contains("cargo clippy") {
            self.steering_after_write_commands
                .push("cargo clippy".to_string());
        }

        if task_input_lower.contains("test") && steering_text.contains("cargo test") {
            self.steering_after_write_commands
                .push("cargo test".to_string());
        }

        if task_input_lower.contains("commit")
            && self.steering_file_contains("git.toml", "git diff --stat")
        {
            self.steering_preflight_commands
                .push("git diff --stat".to_string());
        }

        if task_input_lower.contains("refactor")
            && self.steering_file_contains("code.toml", "cargo clippy")
        {
            self.steering_after_write_commands
                .push("cargo clippy".to_string());
        }

        if task_input_lower.contains("test")
            && self.steering_file_contains("code.toml", "cargo test")
        {
            self.steering_after_write_commands
                .push("cargo test".to_string());
        }

        self.steering_preflight_commands.sort();
        self.steering_preflight_commands.dedup();
        self.steering_after_write_commands.sort();
        self.steering_after_write_commands.dedup();
    }

    fn steering_file_contains(&self, file_name: &str, needle: &str) -> bool {
        self.steering_file_paths(file_name).into_iter().any(|path| {
            std::fs::read_to_string(path)
                .map(|content| {
                    content
                        .to_ascii_lowercase()
                        .contains(&needle.to_ascii_lowercase())
                })
                .unwrap_or(false)
        })
    }

    fn steering_file_paths(&self, file_name: &str) -> Vec<PathBuf> {
        vec![
            self.config
                .core
                .workspace
                .join(".rove/steering")
                .join(file_name),
            self.config.steering.skill_dir.join(file_name),
        ]
    }

    async fn inject_memory_context(
        &self,
        task_input: &str,
        domain: TaskDomain,
        system_prompt: &mut String,
    ) {
        let Some(memory_system) = self.memory_system.as_ref() else {
            return;
        };

        match memory_system.query(task_input, &domain, None).await {
            Ok(hits) => {
                let hits: Vec<_> = hits
                    .into_iter()
                    .filter(|hit| self.hit_matches_workspace(&hit.content))
                    .collect();
                if hits.is_empty() {
                    return;
                }

                let mut memory_context = String::from("\n\n<relevant_memories>\n");
                for hit in &hits {
                    memory_context.push_str(&format!("- [{}] {}\n", hit.source, hit.content));
                }
                memory_context.push_str("</relevant_memories>");
                system_prompt.push_str(&memory_context);
                debug!("Injected {} memory hits into system prompt", hits.len());
            }
            Err(error) => {
                debug!("Memory query failed (non-fatal): {}", error);
            }
        }
    }

    async fn inject_preferences(&self, system_prompt: &mut String) {
        let prefs_block = self.preferences_manager.get_context_block().await;
        if prefs_block.is_empty() {
            return;
        }

        system_prompt.push_str(&prefs_block);
        debug!("Injected user preferences into system prompt");
    }

    fn hit_matches_workspace(&self, content: &str) -> bool {
        let workspace = &self.config.core.workspace;
        for raw_token in content.split_whitespace() {
            let token = raw_token.trim_matches(|ch: char| {
                matches!(
                    ch,
                    '`' | '"' | '\'' | ',' | '.' | ';' | ':' | '(' | ')' | '[' | ']'
                )
            });
            if !token.starts_with('/') {
                continue;
            }

            let path = std::path::Path::new(token);
            if path.is_absolute() && !path.starts_with(workspace) {
                return false;
            }
        }

        true
    }
}
