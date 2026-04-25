use crate::gateway::Task;
use crate::storage::tasks::ApexHistorySummary;
use sdk::{Complexity, TaskDomain};

use super::prompt::TaskContext;
use super::AgentCore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionStrategy {
    Linear,
    Apex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrchestrationDecision {
    pub strategy: ExecutionStrategy,
    pub estimated_steps: usize,
    pub reasons: Vec<String>,
}

impl OrchestrationDecision {
    pub(super) fn use_apex(&self) -> bool {
        matches!(self.strategy, ExecutionStrategy::Apex)
    }

    pub(super) fn summary(&self) -> String {
        let strategy = match self.strategy {
            ExecutionStrategy::Linear => "linear",
            ExecutionStrategy::Apex => "apex",
        };
        if self.reasons.is_empty() {
            return format!("{strategy} · {} step(s)", self.estimated_steps);
        }

        format!(
            "{strategy} · {} step(s) · {}",
            self.estimated_steps,
            self.reasons.join(", ")
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrchestrationHistory {
    pub sampled_tasks: usize,
    pub apex_tasks: usize,
    pub linear_tasks: usize,
    pub failed_tasks: usize,
    pub average_apex_steps: usize,
}

impl AgentCore {
    pub async fn select_execution_strategy(
        &self,
        task: &Task,
        context: &TaskContext,
    ) -> OrchestrationDecision {
        let history = self
            .task_repo
            .get_recent_apex_history(&context.domain_str, 12)
            .await
            .ok()
            .map(|summaries| summarize_history(&summaries))
            .unwrap_or_default();

        decide_execution_strategy(
            task,
            context,
            &self.policy_after_write_commands,
            Some(&history),
        )
    }
}

pub fn decide_execution_strategy(
    task: &Task,
    context: &TaskContext,
    policy_after_write_commands: &[String],
    history: Option<&OrchestrationHistory>,
) -> OrchestrationDecision {
    let lower = format!(" {} ", task.input.to_ascii_lowercase());
    let sequence_markers = count_mentions(
        &lower,
        &[
            " then ",
            " and then ",
            " after ",
            " before ",
            " first ",
            " second ",
            " third ",
            " next ",
            " finally ",
            " once ",
        ],
    );
    let gather_markers = count_mentions(
        &lower,
        &[
            " compare ",
            " gather ",
            " collect ",
            " inspect ",
            " audit ",
            " review ",
            " verify ",
            " summarize ",
            " summarise ",
            " find all ",
            " search ",
        ],
    );
    let coordinated_outputs = count_mentions(
        &lower,
        &[
            " report ",
            " checklist ",
            " summary ",
            " diff ",
            " findings ",
        ],
    );

    let estimated_steps = (1
        + sequence_markers
        + gather_markers.min(2)
        + coordinated_outputs.min(1)
        + usize::from(!policy_after_write_commands.is_empty()))
    .clamp(1, 6);

    let mut score = 0usize;
    let mut reasons = Vec::new();

    match context.complexity {
        Complexity::Complex => {
            reasons.push("complex dispatch".to_string());
            return OrchestrationDecision {
                strategy: ExecutionStrategy::Apex,
                estimated_steps: estimated_steps.max(3),
                reasons,
            };
        }
        Complexity::Medium => {
            score += 2;
            reasons.push("medium dispatch".to_string());
        }
        Complexity::Simple => {}
    }

    if matches!(
        context.domain,
        TaskDomain::Code
            | TaskDomain::Git
            | TaskDomain::Shell
            | TaskDomain::Browser
            | TaskDomain::Data
    ) {
        score += 1;
        reasons.push(format!("{} domain", task_domain_label(context.domain)));
    }

    if task.workspace.is_some()
        && matches!(
            context.domain,
            TaskDomain::Code | TaskDomain::Git | TaskDomain::Shell | TaskDomain::Data
        )
    {
        score += 1;
        reasons.push("workspace-aware execution".to_string());
    }

    if !policy_after_write_commands.is_empty() {
        score += 1;
        reasons.push("post-write verification".to_string());
    }

    if estimated_steps >= 3 {
        score += 2;
        reasons.push(format!("estimated {} steps", estimated_steps));
    }

    if gather_markers > 0 {
        score += 1;
        reasons.push("explicit gather/verify language".to_string());
    }

    if task.input.len() > 220 || task.input.lines().count() > 1 {
        score += 1;
        reasons.push("long multi-part prompt".to_string());
    }

    if let Some(history) = history {
        if history.sampled_tasks >= 2 && history.apex_tasks >= history.linear_tasks.max(1) {
            score += 1;
            reasons.push("recent domain work was multi-step".to_string());
        }

        if history.failed_tasks > 0 {
            score += 1;
            reasons.push("recent domain failures suggest verification".to_string());
        }

        if history.average_apex_steps >= 3 {
            score += 1;
            reasons.push(format!(
                "recent domain tasks averaged {} APEX steps",
                history.average_apex_steps
            ));
        }
    }

    let strategy = if score >= 3 {
        ExecutionStrategy::Apex
    } else {
        ExecutionStrategy::Linear
    };

    OrchestrationDecision {
        strategy,
        estimated_steps,
        reasons,
    }
}

fn summarize_history(summaries: &[ApexHistorySummary]) -> OrchestrationHistory {
    if summaries.is_empty() {
        return OrchestrationHistory::default();
    }

    let apex_tasks = summaries
        .iter()
        .filter(|summary| summary.dag_step_successes > 0 || summary.dag_step_failures > 0)
        .count();
    let failed_tasks = summaries
        .iter()
        .filter(|summary| matches!(summary.status, crate::storage::TaskStatus::Failed))
        .count();
    let apex_step_total: i64 = summaries
        .iter()
        .map(|summary| summary.dag_step_successes + summary.dag_step_failures)
        .sum();
    let average_apex_steps = if apex_tasks == 0 {
        0
    } else {
        ((apex_step_total as f64) / (apex_tasks as f64)).round() as usize
    };

    OrchestrationHistory {
        sampled_tasks: summaries.len(),
        apex_tasks,
        linear_tasks: summaries.len().saturating_sub(apex_tasks),
        failed_tasks,
        average_apex_steps,
    }
}

fn count_mentions(haystack: &str, needles: &[&str]) -> usize {
    needles
        .iter()
        .filter(|needle| haystack.contains(**needle))
        .count()
}

fn task_domain_label(domain: TaskDomain) -> &'static str {
    match domain {
        TaskDomain::Code => "code",
        TaskDomain::Git => "git",
        TaskDomain::Shell => "shell",
        TaskDomain::Browser => "browser",
        TaskDomain::Data => "data",
        TaskDomain::General => "general",
    }
}
