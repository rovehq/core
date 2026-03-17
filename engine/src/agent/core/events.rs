use anyhow::{Context, Result};
use uuid::Uuid;

use crate::llm::ToolCall;

use super::AgentCore;

impl AgentCore {
    pub(super) async fn insert_user_event(
        &self,
        task_id: &Uuid,
        input: &str,
        domain_str: &str,
    ) -> Result<()> {
        let payload = serde_json::json!({ "content": input }).to_string();
        self.task_repo
            .insert_agent_event(task_id, "thought", &payload, 0, Some(domain_str))
            .await
            .context("Failed to persist user message to agent_events")
            .map(|_| ())
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
            "tool_args": tool_call.arguments,
            "tool_id": tool_call.id
        })
        .to_string();

        self.task_repo
            .insert_agent_event(
                task_id,
                "tool_call",
                &payload,
                (iteration * 2 - 1) as i64,
                Some(domain_str),
            )
            .await
            .context("Failed to persist tool call to agent_events")
            .map(|_| ())
    }

    pub(super) async fn insert_observation_event(
        &self,
        task_id: &Uuid,
        observation: &str,
        iteration: usize,
        domain_str: &str,
    ) -> Result<()> {
        let payload = serde_json::json!({ "observation": observation }).to_string();
        self.task_repo
            .insert_agent_event(
                task_id,
                "observation",
                &payload,
                (iteration * 2) as i64,
                Some(domain_str),
            )
            .await
            .context("Failed to persist tool result to agent_events")
            .map(|_| ())
    }

    pub(super) async fn insert_answer_event(
        &self,
        task_id: &Uuid,
        answer: &str,
        iteration: usize,
        domain_str: &str,
    ) -> Result<()> {
        let payload = serde_json::json!({ "answer": answer }).to_string();
        self.task_repo
            .insert_agent_event(
                task_id,
                "answer",
                &payload,
                (iteration * 2 - 1) as i64,
                Some(domain_str),
            )
            .await
            .context("Failed to persist final answer to agent_events")
            .map(|_| ())
    }
}
