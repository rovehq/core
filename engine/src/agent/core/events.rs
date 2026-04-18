use anyhow::{Context, Result};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::llm::ToolCall;
use crate::message_bus::{Event as BusEvent, TaskStreamEvent};
use crate::security::secrets::scrub_text;

use super::AgentCore;

impl AgentCore {
    fn event_timestamp() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_secs() as i64)
            .unwrap_or(0)
    }

    async fn publish_stream_event(&self, task_id: &Uuid, event: TaskStreamEvent) {
        if let Some(bus) = &self.message_bus {
            bus.publish(BusEvent::TaskStream {
                task_id: task_id.to_string(),
                event,
            })
            .await;
        }
    }

    pub(super) async fn publish_turn_start_event(
        &self,
        task_id: &Uuid,
        input: &str,
        task_source: &str,
    ) {
        self.publish_stream_event(
            task_id,
            TaskStreamEvent {
                id: format!("{}:turn_start", task_id),
                task_id: task_id.to_string(),
                phase: "turn_start".to_string(),
                summary: "Task started".to_string(),
                detail: Some(scrub_text(input)),
                raw_event_type: None,
                tool_name: None,
                status: Some("running".to_string()),
                step_num: 0,
                domain: Some(task_source.to_string()),
                created_at: Self::event_timestamp(),
            },
        )
        .await;
    }

    pub(super) async fn publish_turn_end_event(
        &self,
        task_id: &Uuid,
        status: &str,
        summary: &str,
        step_num: i64,
    ) {
        self.publish_stream_event(
            task_id,
            TaskStreamEvent {
                id: format!("{}:turn_end:{}", task_id, status),
                task_id: task_id.to_string(),
                phase: "turn_end".to_string(),
                summary: summary.to_string(),
                detail: None,
                raw_event_type: None,
                tool_name: None,
                status: Some(status.to_string()),
                step_num,
                domain: None,
                created_at: Self::event_timestamp(),
            },
        )
        .await;
    }

    fn tool_step(iteration: usize) -> i64 {
        if iteration == 0 {
            1
        } else {
            (iteration * 2 - 1) as i64
        }
    }

    fn observation_step(iteration: usize) -> i64 {
        if iteration == 0 {
            2
        } else {
            (iteration * 2) as i64
        }
    }

    fn evaluation_step(iteration: usize, attempt: usize) -> i64 {
        (iteration * 100 + attempt.max(1)) as i64
    }

    pub(super) async fn insert_user_event(
        &self,
        task_id: &Uuid,
        input: &str,
        domain_str: &str,
    ) -> Result<()> {
        let payload = serde_json::json!({ "content": scrub_text(input) }).to_string();
        let event_id = self
            .task_repo
            .insert_agent_event(task_id, "thought", &payload, 0, Some(domain_str))
            .await
            .context("Failed to persist user message to agent_events")?;
        self.publish_stream_event(
            task_id,
            TaskStreamEvent {
                id: event_id,
                task_id: task_id.to_string(),
                phase: "thought".to_string(),
                summary: "User input".to_string(),
                detail: Some(scrub_text(input)),
                raw_event_type: Some("thought".to_string()),
                tool_name: None,
                status: None,
                step_num: 0,
                domain: Some(domain_str.to_string()),
                created_at: Self::event_timestamp(),
            },
        )
        .await;
        Ok(())
    }

    pub(super) async fn insert_thought_event(
        &self,
        task_id: &Uuid,
        thought: &str,
        domain_str: &str,
    ) -> Result<()> {
        let payload = serde_json::json!({ "content": scrub_text(thought) }).to_string();
        let event_id = self
            .task_repo
            .insert_agent_event(task_id, "thought", &payload, 0, Some(domain_str))
            .await
            .context("Failed to persist thought to agent_events")?;
        self.publish_stream_event(
            task_id,
            TaskStreamEvent {
                id: event_id,
                task_id: task_id.to_string(),
                phase: "thought".to_string(),
                summary: "Reasoning".to_string(),
                detail: Some(scrub_text(thought)),
                raw_event_type: Some("thought".to_string()),
                tool_name: None,
                status: None,
                step_num: 0,
                domain: Some(domain_str.to_string()),
                created_at: Self::event_timestamp(),
            },
        )
        .await;
        Ok(())
    }

    pub(super) async fn insert_tool_call_event(
        &self,
        task_id: &Uuid,
        tool_call: &ToolCall,
        iteration: usize,
        domain_str: &str,
    ) -> Result<()> {
        let payload = serde_json::json!({
            "tool_name": tool_call.name,
            "tool_args": scrub_text(&tool_call.arguments),
            "tool_id": tool_call.id
        })
        .to_string();

        let step_num = Self::tool_step(iteration);
        let event_id = self
            .task_repo
            .insert_agent_event(task_id, "tool_call", &payload, step_num, Some(domain_str))
            .await
            .context("Failed to persist tool call to agent_events")?;
        self.publish_stream_event(
            task_id,
            TaskStreamEvent {
                id: event_id,
                task_id: task_id.to_string(),
                phase: "tool_use".to_string(),
                summary: format!("Tool call: {}", tool_call.name),
                detail: Some(scrub_text(&tool_call.arguments)),
                raw_event_type: Some("tool_call".to_string()),
                tool_name: Some(tool_call.name.clone()),
                status: None,
                step_num,
                domain: Some(domain_str.to_string()),
                created_at: Self::event_timestamp(),
            },
        )
        .await;
        Ok(())
    }

    pub(super) async fn insert_observation_event(
        &self,
        task_id: &Uuid,
        observation: &str,
        iteration: usize,
        domain_str: &str,
    ) -> Result<()> {
        let payload = serde_json::json!({ "observation": scrub_text(observation) }).to_string();
        let step_num = Self::observation_step(iteration);
        let event_id = self
            .task_repo
            .insert_agent_event(task_id, "observation", &payload, step_num, Some(domain_str))
            .await
            .context("Failed to persist tool result to agent_events")?;
        self.publish_stream_event(
            task_id,
            TaskStreamEvent {
                id: event_id,
                task_id: task_id.to_string(),
                phase: "tool_result".to_string(),
                summary: "Tool result".to_string(),
                detail: Some(scrub_text(observation)),
                raw_event_type: Some("observation".to_string()),
                tool_name: None,
                status: None,
                step_num,
                domain: Some(domain_str.to_string()),
                created_at: Self::event_timestamp(),
            },
        )
        .await;
        Ok(())
    }

    pub(super) async fn insert_answer_event(
        &self,
        task_id: &Uuid,
        answer: &str,
        iteration: usize,
        domain_str: &str,
    ) -> Result<()> {
        let payload = serde_json::json!({ "answer": scrub_text(answer) }).to_string();
        let step_num = Self::tool_step(iteration);
        let event_id = self
            .task_repo
            .insert_agent_event(task_id, "answer", &payload, step_num, Some(domain_str))
            .await
            .context("Failed to persist final answer to agent_events")?;
        self.publish_stream_event(
            task_id,
            TaskStreamEvent {
                id: event_id,
                task_id: task_id.to_string(),
                phase: "final_answer".to_string(),
                summary: "Final answer".to_string(),
                detail: Some(scrub_text(answer)),
                raw_event_type: Some("answer".to_string()),
                tool_name: None,
                status: None,
                step_num,
                domain: Some(domain_str.to_string()),
                created_at: Self::event_timestamp(),
            },
        )
        .await;
        Ok(())
    }

    pub(super) async fn insert_error_event(
        &self,
        task_id: &Uuid,
        error: &str,
        domain_str: &str,
    ) -> Result<()> {
        let payload = serde_json::json!({ "error": scrub_text(error) }).to_string();
        let event_id = self
            .task_repo
            .insert_agent_event(task_id, "error", &payload, -1, Some(domain_str))
            .await
            .context("Failed to persist error to agent_events")?;
        self.publish_stream_event(
            task_id,
            TaskStreamEvent {
                id: event_id,
                task_id: task_id.to_string(),
                phase: "error".to_string(),
                summary: "Execution error".to_string(),
                detail: Some(scrub_text(error)),
                raw_event_type: Some("error".to_string()),
                tool_name: None,
                status: Some("failed".to_string()),
                step_num: -1,
                domain: if domain_str.is_empty() {
                    None
                } else {
                    Some(domain_str.to_string())
                },
                created_at: Self::event_timestamp(),
            },
        )
        .await;
        Ok(())
    }

    pub(super) async fn insert_evaluation_started_event(
        &self,
        task_id: &Uuid,
        answer: &str,
        iteration: usize,
        attempt: usize,
        domain_str: &str,
    ) -> Result<()> {
        let payload = serde_json::json!({
            "answer": scrub_text(answer),
            "attempt": attempt
        })
        .to_string();
        let step_num = Self::evaluation_step(iteration, attempt);
        let event_id = self
            .task_repo
            .insert_agent_event(
                task_id,
                "evaluation_started",
                &payload,
                step_num,
                Some(domain_str),
            )
            .await
            .context("Failed to persist evaluation start to agent_events")?;
        self.publish_stream_event(
            task_id,
            TaskStreamEvent {
                id: event_id,
                task_id: task_id.to_string(),
                phase: "evaluation_start".to_string(),
                summary: "Outcome evaluation started".to_string(),
                detail: Some(scrub_text(answer)),
                raw_event_type: Some("evaluation_started".to_string()),
                tool_name: None,
                status: Some("running".to_string()),
                step_num,
                domain: Some(domain_str.to_string()),
                created_at: Self::event_timestamp(),
            },
        )
        .await;
        Ok(())
    }

    pub(super) async fn insert_evaluation_result_event(
        &self,
        task_id: &Uuid,
        decision: &str,
        reason: &str,
        iteration: usize,
        attempt: usize,
        domain_str: &str,
    ) -> Result<()> {
        let payload = serde_json::json!({
            "decision": decision,
            "reason": scrub_text(reason),
            "attempt": attempt
        })
        .to_string();
        let step_num = Self::evaluation_step(iteration, attempt);
        let event_id = self
            .task_repo
            .insert_agent_event(
                task_id,
                "evaluation_result",
                &payload,
                step_num,
                Some(domain_str),
            )
            .await
            .context("Failed to persist evaluation result to agent_events")?;
        self.publish_stream_event(
            task_id,
            TaskStreamEvent {
                id: event_id,
                task_id: task_id.to_string(),
                phase: "evaluation_result".to_string(),
                summary: format!("Outcome evaluation: {}", decision),
                detail: Some(scrub_text(reason)),
                raw_event_type: Some("evaluation_result".to_string()),
                tool_name: None,
                status: Some(decision.to_string()),
                step_num,
                domain: Some(domain_str.to_string()),
                created_at: Self::event_timestamp(),
            },
        )
        .await;
        Ok(())
    }

    pub(super) async fn insert_evaluation_retry_event(
        &self,
        task_id: &Uuid,
        guidance: &str,
        iteration: usize,
        attempt: usize,
        domain_str: &str,
    ) -> Result<()> {
        let payload = serde_json::json!({
            "guidance": scrub_text(guidance),
            "attempt": attempt
        })
        .to_string();
        let step_num = Self::evaluation_step(iteration, attempt);
        let event_id = self
            .task_repo
            .insert_agent_event(
                task_id,
                "evaluation_retry",
                &payload,
                step_num,
                Some(domain_str),
            )
            .await
            .context("Failed to persist evaluation retry to agent_events")?;
        self.publish_stream_event(
            task_id,
            TaskStreamEvent {
                id: event_id,
                task_id: task_id.to_string(),
                phase: "evaluation_retry".to_string(),
                summary: "Outcome evaluation requested retry".to_string(),
                detail: Some(scrub_text(guidance)),
                raw_event_type: Some("evaluation_retry".to_string()),
                tool_name: None,
                status: Some("retry".to_string()),
                step_num,
                domain: Some(domain_str.to_string()),
                created_at: Self::event_timestamp(),
            },
        )
        .await;
        Ok(())
    }
}
