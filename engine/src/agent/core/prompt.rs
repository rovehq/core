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

pub struct TaskContext {
    pub domain_str: String,
    pub domain: TaskDomain,
    pub complexity: Complexity,
    pub route: Route,
    pub sensitive: bool,
}

impl AgentCore {
    pub async fn initialize_task_context(
        &mut self,
        task: &Task,
        risk_tier: RiskTier,
    ) -> Result<TaskContext> {
        self.memory.clear();
        self.current_execution_profile = task.execution_profile.clone();
        self.current_callable_roster = task
            .execution_profile
            .as_ref()
            .map(|p| p.callable_agents.clone())
            .unwrap_or_default();
        self.policy_preflight_commands.clear();
        self.policy_after_write_commands.clear();
        self.policy_executed_commands.clear();
        let mut system_prompt = self.tools.system_prompt_for_query(&task.input);
        self.inject_execution_profile(&mut system_prompt);
        self.inject_callable_roster(&mut system_prompt);
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

        if let Some(thread_context) = self
            .current_execution_profile
            .as_ref()
            .and_then(|profile| profile.thread_id.as_deref())
        {
            let task_id = task.id.to_string();
            let history = self
                .task_repo
                .get_recent_thread_history(thread_context, Some(task_id.as_str()), 3)
                .await
                .unwrap_or_default();
            if !history.is_empty() {
                let mut lines = vec![format!(
                    "Persistent execution thread context ({}):",
                    thread_context
                )];
                for entry in history {
                    lines.push(format!("Previous user task: {}", entry.input.trim()));
                    if let Some(answer) = entry
                        .answer
                        .as_deref()
                        .filter(|value| !value.trim().is_empty())
                    {
                        lines.push(format!("Previous answer: {}", answer.trim()));
                    }
                }
                system_prompt = format!("{}\n\n{}", lines.join("\n"), system_prompt);
            }
        }

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

        self.current_domain = dispatch_result.domain;
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

    fn inject_execution_profile(&self, system_prompt: &mut String) {
        let Some(profile) = self.current_execution_profile.as_ref() else {
            return;
        };
        if profile.instructions.trim().is_empty()
            && profile.allowed_tools.is_empty()
            && profile.output_contract.is_none()
            && profile.outcome_contract.is_none()
            && profile.max_iterations.is_none()
            && profile.agent_name.is_none()
            && profile.worker_preset_name.is_none()
            && profile.purpose.is_none()
        {
            return;
        }

        let mut lines = Vec::new();
        lines.push("You are executing under a saved Rove agent profile.".to_string());
        if let Some(agent_name) = profile.agent_name.as_ref() {
            lines.push(format!("Agent: {}", agent_name));
        }
        if let Some(worker_name) = profile
            .worker_preset_name
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            lines.push(format!("Worker preset: {}", worker_name));
        }
        if let Some(purpose) = profile
            .purpose
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            lines.push(format!("Purpose: {}", purpose.trim()));
        }
        if !profile.instructions.trim().is_empty() {
            lines.push(String::new());
            lines.push("Agent instructions:".to_string());
            lines.push(profile.instructions.trim().to_string());
        }
        if !profile.allowed_tools.is_empty() {
            lines.push(String::new());
            lines.push(format!(
                "Allowed tools: {}",
                profile.allowed_tools.join(", ")
            ));
            lines.push(
                "Do not call tools outside that set. If you need a missing capability, explain the blocker."
                    .to_string(),
            );
        }
        if let Some(output_contract) = profile
            .output_contract
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            lines.push(String::new());
            lines.push("Output contract:".to_string());
            lines.push(output_contract.trim().to_string());
        }
        if let Some(outcome_contract) = profile
            .outcome_contract
            .as_ref()
            .filter(|value| !value.success_criteria.trim().is_empty())
        {
            lines.push(String::new());
            lines.push("Outcome contract:".to_string());
            lines.push(format!(
                "Success criteria: {}",
                outcome_contract.success_criteria.trim()
            ));
            lines.push(format!(
                "Self-evaluation policy: {} (max retries: {}).",
                outcome_contract.evaluator_policy.trim(),
                outcome_contract.max_self_evals
            ));
            lines.push(
                "Before settling on the final answer, verify it satisfies the success criteria exactly."
                    .to_string(),
            );
        }
        if let Some(max_iterations) = profile.max_iterations.filter(|value| *value > 0) {
            lines.push(String::new());
            lines.push(format!(
                "Iteration bound: finish within {} reasoning iteration(s).",
                max_iterations
            ));
        }

        *system_prompt = format!("{}\n\n{}", lines.join("\n"), system_prompt);
    }

    fn inject_callable_roster(&self, system_prompt: &mut String) {
        if self.current_callable_roster.is_empty() {
            return;
        }
        let roster_lines: Vec<String> = self
            .current_callable_roster
            .iter()
            .map(|ca| {
                format!(
                    "  - id: \"{}\", name: \"{}\", role: \"{}\"",
                    ca.id, ca.name, ca.role
                )
            })
            .collect();

        // If no other tools are registered, the schema builder never emits the
        // IMPORTANT RULES / tool-call format block.  Inject it here so the LLM
        // knows the JSON call format even when call_agent is the only tool.
        if !system_prompt.contains("IMPORTANT RULES") {
            system_prompt.push_str("\n\nIMPORTANT RULES:\n");
            system_prompt.push_str("1. To call a tool, your ENTIRE response must be ONLY the JSON object — nothing else. No explanation, no markdown fences, no text before or after.\n");
            system_prompt.push_str("2. When you have the final answer (after receiving tool results), respond with plain text only — no JSON.\n");
            system_prompt
                .push_str("\nTool call format (your entire response must be exactly this):\n");
            system_prompt.push_str(r#"{"function": "tool_name", "arguments": {"arg1": "value1"}}"#);
            system_prompt.push_str("\n\nAvailable tools:");
        }

        // Use the same ## tool_name / Arguments: format as the builtin tools so the
        // LLM knows exactly what JSON to emit.  The roster list tells it which
        // agent_id values are valid.
        let section = format!(
            "\n\n## call_agent\nDelegate a sub-task to a named bounded agent declared in your callable-agent roster.\nArguments: {{\"agent_id\": \"<id from roster>\", \"prompt\": \"<task for the agent>\"}}\nCallable agent roster:\n{}\n",
            roster_lines.join("\n")
        );
        system_prompt.push_str(&section);
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

        match memory_system
            .build_context_bundle(task_input, &domain, None, Some(&self.config.core.workspace))
            .await
        {
            Ok(bundle) => {
                let mut sections = Vec::new();

                let fact_lines: Vec<_> = bundle
                    .facts
                    .iter()
                    .chain(bundle.preferences.iter())
                    .chain(bundle.warnings.iter())
                    .chain(bundle.errors.iter())
                    .filter(|hit| self.hit_matches_workspace(&hit.content))
                    .map(|hit| format!("- [{}] {}", hit.source, hit.content))
                    .collect();
                if !fact_lines.is_empty() {
                    sections.push(format!("typed\n{}", fact_lines.join("\n")));
                }

                let graph_lines: Vec<_> = bundle
                    .graph_paths
                    .iter()
                    .filter(|path| self.hit_matches_workspace(&path.summary))
                    .map(|path| format!("- {} [{}]", path.summary, path.source_kinds.join(", ")))
                    .collect();
                if !graph_lines.is_empty() {
                    sections.push(format!("graph\n{}", graph_lines.join("\n")));
                }

                let semantic_lines: Vec<_> = bundle
                    .flattened_hits()
                    .into_iter()
                    .filter(|hit| {
                        matches!(
                            hit.hit_type,
                            crate::conductor::HitType::Episodic
                                | crate::conductor::HitType::Insight
                                | crate::conductor::HitType::TaskTrace
                        )
                    })
                    .filter(|hit| self.hit_matches_workspace(&hit.content))
                    .map(|hit| format!("- [{}] {}", hit.source, hit.content))
                    .collect();
                if !semantic_lines.is_empty() {
                    sections.push(format!("semantic\n{}", semantic_lines.join("\n")));
                }

                if sections.is_empty() {
                    return;
                }

                let mut memory_context = String::from("\n\n<relevant_memories>\n");
                for section in sections {
                    memory_context.push_str(&format!("{}\n", section));
                }
                memory_context.push_str("</relevant_memories>");
                system_prompt.push_str(&memory_context);
                debug!("Injected structured memory bundle into system prompt");
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
