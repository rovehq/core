use anyhow::Result;
use tracing::debug;

use crate::gateway::Task;
use crate::llm::Message;
use crate::risk_assessor::RiskTier;
use sdk::TaskDomain;

use super::AgentCore;

pub(super) struct TaskContext {
    pub(super) domain_str: String,
}

impl AgentCore {
    pub(super) async fn initialize_task_context(
        &mut self,
        task: &Task,
        risk_tier: RiskTier,
    ) -> Result<TaskContext> {
        self.memory.clear();
        let mut system_prompt = self.tools.system_prompt();

        self.apply_steering(&task.input, risk_tier, &mut system_prompt)
            .await;

        let dispatch_result = self.dispatch_brain.classify(&task.input);
        let domain_str = dispatch_result.domain.to_string();

        debug!(
            task_id = %task.id,
            domain = ?dispatch_result.domain,
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

        Ok(TaskContext { domain_str })
    }

    async fn apply_steering(
        &mut self,
        task_input: &str,
        risk_tier: RiskTier,
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
        let _ = steering.auto_activate(task_input, risk_tier_u8).await;

        let directives = steering.get_directives().await;
        if !directives.system_prefix.is_empty() {
            *system_prompt = format!("{}\n\n{}", directives.system_prefix, system_prompt);
        }
        if !directives.system_suffix.is_empty() {
            *system_prompt = format!("{}\n\n{}", system_prompt, directives.system_suffix);
        }

        debug!(
            "Active steering directives: {:?}",
            steering.active_skills().await
        );
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
            Ok(hits) if !hits.is_empty() => {
                let mut memory_context = String::from("\n\n<relevant_memories>\n");
                for hit in &hits {
                    memory_context.push_str(&format!("- [{}] {}\n", hit.source, hit.content));
                }
                memory_context.push_str("</relevant_memories>");
                system_prompt.push_str(&memory_context);
                debug!("Injected {} memory hits into system prompt", hits.len());
            }
            Ok(_) => {}
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
}
