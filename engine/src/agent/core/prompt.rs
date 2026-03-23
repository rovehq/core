use anyhow::Result;
use std::path::PathBuf;
use tracing::debug;

use crate::gateway::Task;
use crate::llm::Message;
use crate::policy::types::MergedDirectives;
use crate::policy::{
    active_workspace_policy_dir, legacy_policy_workspace_dir, policy_workspace_dir,
};
use crate::risk_assessor::RiskTier;
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
        self.policy_preflight_commands.clear();
        self.policy_after_write_commands.clear();
        self.policy_executed_commands.clear();
        let mut system_prompt = self.tools.system_prompt_for_query(&task.input);
        let dispatch_result = self.dispatch_brain.classify(&task.input);
        let domain_name = dispatch_result.domain_label.to_lowercase();
        self.current_task_sensitive = dispatch_result.sensitive;

        self.apply_policy(
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

    async fn apply_policy(
        &mut self,
        task_input: &str,
        risk_tier: RiskTier,
        domain: Option<&str>,
        system_prompt: &mut String,
    ) {
        let Some(policy_engine) = self.policy_engine.as_mut() else {
            return;
        };

        let risk_tier_u8 = match risk_tier {
            RiskTier::Tier0 => 0,
            RiskTier::Tier1 => 1,
            RiskTier::Tier2 => 2,
        };
        policy_engine
            .auto_activate_policies(task_input, risk_tier_u8, domain)
            .await;
        let directives = policy_engine.get_directives_for_task(task_input).await;
        let active_policies = policy_engine.active_policies().await;
        let matched_hints = policy_engine.matched_hints(task_input).await;
        let _ = policy_engine;

        self.configure_policy_commands(task_input, &directives, &matched_hints);
        if !directives.system_prefix.is_empty() {
            *system_prompt = format!("{}\n\n{}", directives.system_prefix, system_prompt);
        }
        if !directives.system_suffix.is_empty() {
            *system_prompt = format!("{}\n\n{}", system_prompt, directives.system_suffix);
        }

        debug!("Active policy directives: {:?}", active_policies);
    }

    fn configure_policy_commands(
        &mut self,
        task_input: &str,
        directives: &MergedDirectives,
        matched_hints: &[String],
    ) {
        let task_input_lower = task_input.to_ascii_lowercase();
        let policy_text = format!(
            "{}\n{}\n{}",
            directives.system_prefix,
            directives.system_suffix,
            matched_hints.join("\n")
        )
        .to_ascii_lowercase();

        if task_input_lower.contains("commit") && policy_text.contains("git diff --stat") {
            self.policy_preflight_commands
                .push("git diff --stat".to_string());
        }

        if task_input_lower.contains("refactor") && policy_text.contains("cargo clippy") {
            self.policy_after_write_commands
                .push("cargo clippy".to_string());
        }

        if task_input_lower.contains("test") && policy_text.contains("cargo test") {
            self.policy_after_write_commands
                .push("cargo test".to_string());
        }

        if task_input_lower.contains("commit")
            && self.policy_file_contains("git.toml", "git diff --stat")
        {
            self.policy_preflight_commands
                .push("git diff --stat".to_string());
        }

        if task_input_lower.contains("refactor")
            && self.policy_file_contains("code.toml", "cargo clippy")
        {
            self.policy_after_write_commands
                .push("cargo clippy".to_string());
        }

        if task_input_lower.contains("test") && self.policy_file_contains("code.toml", "cargo test")
        {
            self.policy_after_write_commands
                .push("cargo test".to_string());
        }

        self.policy_preflight_commands.sort();
        self.policy_preflight_commands.dedup();
        self.policy_after_write_commands.sort();
        self.policy_after_write_commands.dedup();
    }

    fn policy_file_contains(&self, file_name: &str, needle: &str) -> bool {
        self.policy_file_paths(file_name).into_iter().any(|path| {
            std::fs::read_to_string(path)
                .map(|content| {
                    content
                        .to_ascii_lowercase()
                        .contains(&needle.to_ascii_lowercase())
                })
                .unwrap_or(false)
        })
    }

    fn policy_file_paths(&self, file_name: &str) -> Vec<PathBuf> {
        let workspace_policy_dir = policy_workspace_dir(&self.config.core.workspace);
        let legacy_workspace_dir = legacy_policy_workspace_dir(&self.config.core.workspace);
        let active_workspace_dir =
            active_workspace_policy_dir(&workspace_policy_dir, &legacy_workspace_dir);
        vec![
            active_workspace_dir.join(file_name),
            workspace_policy_dir.join(file_name),
            legacy_workspace_dir.join(file_name),
            self.config.policy.policy_dir().join(file_name),
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
