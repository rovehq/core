use anyhow::{Context, Result};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::time::timeout;
use tracing::{debug, error, warn};

use crate::gateway::Task;
use crate::llm::{LLMResponse, ToolCall};
use crate::risk_assessor::Operation;
use crate::security::secrets::scrub_text;
use sdk::errors::EngineError;

use super::{AgentCore, TaskResult, LLM_TIMEOUT_SECS, MAX_RESULT_SIZE};

impl AgentCore {
    pub(super) async fn execute_task_loop(
        &mut self,
        task_id: &uuid::Uuid,
        task: Task,
    ) -> Result<TaskResult> {
        let start_time = Instant::now();
        let task_id_str = task_id.to_string();
        let mut tool_call_counts: HashMap<u64, u32> = HashMap::new();

        self.current_source = task.source.clone().into();

        let operation = Operation::new("execute_task", vec![], task.source.clone().into());
        let mut risk_tier = self
            .risk_assessor
            .assess(&operation)
            .context("Failed to assess risk tier")?;

        if let Some(override_tier) = task.risk_tier_override {
            risk_tier = override_tier;
        }

        debug!("Task {} assessed as {:?}", task_id, risk_tier);

        self.rate_limiter
            .check_limit(&task_id_str, risk_tier)
            .await
            .context("Rate limit exceeded")?;
        self.rate_limiter
            .record_operation(&task_id_str, risk_tier)
            .await
            .context("Failed to record operation")?;

        let context = self.initialize_task_context(&task, risk_tier).await?;
        self.insert_user_event(task_id, &task.input, &context.domain_str)
            .await?;

        let max_iterations = if self.config.agent.max_iterations == 0 {
            usize::MAX
        } else {
            self.config.agent.max_iterations as usize
        };
        let confirm_after = self.config.agent.confirm_after as usize;
        let mut iteration = 0;

        while iteration < max_iterations {
            iteration += 1;
            self.confirm_continuation(task_id, iteration, confirm_after)
                .await?;

            debug!(
                "Task {} iteration {}/{}",
                task_id,
                iteration,
                if max_iterations == usize::MAX {
                    "∞".to_string()
                } else {
                    max_iterations.to_string()
                }
            );

            let (response, provider_name) = self.call_llm(task_id, iteration).await?;
            match response {
                LLMResponse::ToolCall(tool_call) => {
                    self.record_tool_call(task_id, iteration, &mut tool_call_counts, &tool_call)?;

                    self.memory
                        .add_message(self.assistant_tool_message(task_id, &tool_call));
                    self.insert_tool_call_event(
                        task_id,
                        &tool_call,
                        iteration,
                        &context.domain_str,
                    )
                    .await?;

                    let execution = self.execute_tool_call(&task_id_str, &tool_call).await?;
                    self.memory.add_message(crate::llm::Message::tool_result(
                        &execution.safe_result,
                        &tool_call.id,
                    ));
                    self.insert_observation_event(
                        task_id,
                        &execution.safe_result,
                        iteration,
                        &context.domain_str,
                    )
                    .await?;

                    debug!(
                        task_id = %task_id,
                        iteration,
                        provider = %provider_name,
                        tool = %tool_call.name,
                        "Tool execution completed"
                    );
                }
                LLMResponse::FinalAnswer(answer) => {
                    debug!(task_id = %task_id, iteration, "Final answer received");

                    if answer.content.len() > MAX_RESULT_SIZE {
                        warn!(
                            task_id = %task_id,
                            iteration,
                            size = answer.content.len(),
                            limit = MAX_RESULT_SIZE,
                            "Final answer exceeds size limit: {} bytes > {} bytes",
                            answer.content.len(),
                            MAX_RESULT_SIZE
                        );
                        return Err(EngineError::ResultSizeExceeded {
                            size: answer.content.len(),
                            limit: MAX_RESULT_SIZE,
                        }
                        .into());
                    }

                    self.insert_answer_event(
                        task_id,
                        &answer.content,
                        iteration,
                        &context.domain_str,
                    )
                    .await?;

                    return Ok(TaskResult::success(
                        task_id.to_string(),
                        answer.content,
                        provider_name,
                        start_time.elapsed().as_millis() as i64,
                        iteration,
                    ));
                }
            }
        }

        error!(
            task_id = %task_id,
            max_iterations = if max_iterations == usize::MAX {
                "∞".to_string()
            } else {
                max_iterations.to_string()
            },
            "Task exceeded max iterations"
        );
        Err(EngineError::MaxIterationsExceeded.into())
    }

    async fn confirm_continuation(
        &self,
        task_id: &uuid::Uuid,
        iteration: usize,
        confirm_after: usize,
    ) -> Result<()> {
        if confirm_after == 0 || iteration <= 1 || !(iteration - 1).is_multiple_of(confirm_after) {
            return Ok(());
        }

        println!(
            "\n[Agent Pause] Reached {} iterations without task completion.",
            iteration - 1
        );
        println!("To continue working on this task, press ENTER. To abort, type 'abort' and press ENTER.");

        let mut reader = BufReader::new(tokio::io::stdin());
        let mut buf = String::new();
        let _ = reader.read_line(&mut buf).await;
        if buf.trim().eq_ignore_ascii_case("abort") {
            warn!(
                task_id = %task_id,
                iteration = iteration - 1,
                "Agent execution aborted by user after {} iterations",
                iteration - 1
            );
            return Err(EngineError::OperationAbortedByUser.into());
        }

        println!("Continuing execution...");
        Ok(())
    }

    async fn call_llm(
        &self,
        task_id: &uuid::Uuid,
        iteration: usize,
    ) -> Result<(LLMResponse, String)> {
        let llm_result = timeout(
            Duration::from_secs(LLM_TIMEOUT_SECS),
            self.router.call(self.memory.messages()),
        )
        .await;

        match llm_result {
            Ok(Ok((response, provider))) => Ok((response, provider)),
            Ok(Err(error)) => {
                error!(
                    task_id = %task_id,
                    iteration,
                    "LLM call failed: {}",
                    scrub_text(&error.to_string())
                );
                Err(error.into())
            }
            Err(_) => {
                error!(
                    task_id = %task_id,
                    iteration,
                    timeout_secs = LLM_TIMEOUT_SECS,
                    "LLM call timed out after {}s",
                    LLM_TIMEOUT_SECS
                );
                Err(EngineError::LLMTimeout.into())
            }
        }
    }

    fn record_tool_call(
        &self,
        task_id: &uuid::Uuid,
        iteration: usize,
        tool_call_counts: &mut HashMap<u64, u32>,
        tool_call: &ToolCall,
    ) -> Result<()> {
        debug!(
            task_id = %task_id,
            iteration,
            tool = %tool_call.name,
            tool_id = %tool_call.id,
            "Tool call: {} ({})",
            tool_call.name,
            tool_call.id
        );

        let mut hasher = DefaultHasher::new();
        tool_call.name.hash(&mut hasher);
        tool_call.arguments.hash(&mut hasher);
        let count = tool_call_counts.entry(hasher.finish()).or_insert(0);
        *count += 1;
        if *count >= 3 {
            let error = format!(
                "Tool '{}' with identical arguments called 3 times.",
                tool_call.name
            );
            warn!(
                task_id = %task_id,
                iteration,
                tool = %tool_call.name,
                "Infinite loop detected: {}",
                error
            );
            return Err(EngineError::InfiniteLoopDetected(error).into());
        }

        Ok(())
    }
}
