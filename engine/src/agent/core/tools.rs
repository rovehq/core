use anyhow::{Context, Result};
use sha2::Digest;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{debug, info, warn};

use crate::llm::{Message, ToolCall};
use crate::risk_assessor::{classify_terminal_command, Operation, RiskTier};
use crate::security::secrets::scrub_text;
use sdk::errors::EngineError;

use super::{AgentCore, MAX_RESULT_SIZE};

pub(super) struct ToolExecution {
    pub(super) safe_result: String,
}

impl AgentCore {
    pub(super) fn assistant_tool_message(
        &self,
        task_id: &uuid::Uuid,
        tool_call: &ToolCall,
    ) -> Message {
        let arguments = self.parse_tool_arguments(&task_id.to_string(), tool_call);
        Message::assistant(
            serde_json::json!({
                "function": &tool_call.name,
                "arguments": arguments
            })
            .to_string(),
        )
    }

    pub(super) async fn execute_tool_call(
        &self,
        task_id: &str,
        tool_call: &ToolCall,
    ) -> Result<ToolExecution> {
        let tool_args = self.parse_tool_arguments(task_id, tool_call);
        let (tool_tier, approved_by) = self
            .assess_tool_risk(task_id, &tool_call.name, &tool_args)
            .await?;

        let tool_result = self
            .dispatch_tool(&tool_call.name, &tool_call.arguments)
            .await;
        if tool_result.len() > MAX_RESULT_SIZE {
            warn!(
                task_id = %task_id,
                tool = %tool_call.name,
                size = tool_result.len(),
                limit = MAX_RESULT_SIZE,
                "Tool result exceeds size limit: {} bytes > {} bytes",
                tool_result.len(),
                MAX_RESULT_SIZE
            );
            return Err(EngineError::ResultSizeExceeded {
                size: tool_result.len(),
                limit: MAX_RESULT_SIZE,
            }
            .into());
        }

        let safe_result = self.injection_detector.sanitize(&tool_result);
        self.record_tool_audit(task_id, tool_call, tool_tier, &approved_by, &safe_result)
            .await;

        Ok(ToolExecution { safe_result })
    }

    fn parse_tool_arguments(&self, task_id: &str, tool_call: &ToolCall) -> serde_json::Value {
        serde_json::from_str(&tool_call.arguments).unwrap_or_else(|error| {
            warn!(
                task_id = %task_id,
                tool = %tool_call.name,
                "Failed to parse tool arguments for '{}': {}",
                tool_call.name,
                error
            );
            serde_json::json!({})
        })
    }

    async fn dispatch_tool(&self, tool_name: &str, arguments_json: &str) -> String {
        if tool_name == "write_file" {
            if let Some(workspace) = self.tools.workspace() {
                let lock = self.workspace_locks.get_lock(workspace);
                let _guard = lock.lock().await;
                return self.tools.dispatch(tool_name, arguments_json).await;
            }
        }

        self.tools.dispatch(tool_name, arguments_json).await
    }

    async fn record_tool_audit(
        &self,
        task_id: &str,
        tool_call: &ToolCall,
        tool_tier: i32,
        approved_by: &str,
        safe_result: &str,
    ) {
        let mut hasher = sha2::Sha256::new();
        hasher.update(tool_call.arguments.as_bytes());
        let args_hash = hex::encode(hasher.finalize());

        let result_summary = if safe_result.len() > 100 {
            format!("{}...", &safe_result[0..97])
        } else {
            safe_result.to_string()
        };
        let result_summary = scrub_text(&result_summary);

        if let Err(error) = self
            .task_repo
            .insert_agent_action(
                task_id,
                "tool_execution",
                &tool_call.name,
                &args_hash,
                tool_tier,
                approved_by,
                &result_summary,
            )
            .await
        {
            warn!(task_id = %task_id, "Failed to write to audit log: {}", error);
        }
    }

    async fn assess_tool_risk(
        &self,
        task_id: &str,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> Result<(i32, String)> {
        let op_name = self.tool_operation_name(tool_name, args);

        let arg_strings = match args {
            serde_json::Value::Object(map) => map
                .values()
                .filter_map(|value| value.as_str().map(ToOwned::to_owned))
                .collect(),
            _ => Vec::new(),
        };

        let operation = Operation::new(op_name, arg_strings, self.current_source.clone());
        let tier = self
            .risk_assessor
            .assess(&operation)
            .context("Failed to assess tool risk")?;

        debug!(task_id = %task_id, tool = %tool_name, "Tool '{}' assessed as {:?}", tool_name, tier);

        let approved_by = match tier {
            RiskTier::Tier0 => "auto".to_string(),
            RiskTier::Tier1 => self.confirm_tier1(task_id, tool_name).await,
            RiskTier::Tier2 => self.confirm_tier2(task_id, tool_name).await?,
        };

        let tier_value = match tier {
            RiskTier::Tier0 => 0,
            RiskTier::Tier1 => 1,
            RiskTier::Tier2 => 2,
        };

        Ok((tier_value, approved_by))
    }

    fn tool_operation_name(&self, tool_name: &str, args: &serde_json::Value) -> &'static str {
        match tool_name {
            "read_file" | "list_dir" | "file_exists" => "read_file",
            "write_file" => "write_file",
            "delete_file" => "delete_file",
            "run_command" => args
                .get("command")
                .and_then(|value| value.as_str())
                .map(classify_terminal_command)
                .unwrap_or("execute_command"),
            "capture_screen" => "read_file",
            _ => "execute_task",
        }
    }

    async fn confirm_tier1(&self, task_id: &str, tool_name: &str) -> String {
        if !self.config.security.confirm_tier1 {
            debug!(
                task_id = %task_id,
                tool = %tool_name,
                "Tier 1 operation auto-approved (confirm_tier1=false)"
            );
            return "auto".to_string();
        }

        info!(
            task_id = %task_id,
            tool = %tool_name,
            "Tier 1 operation: {} (write/reversible)",
            tool_name
        );
        println!("\n[Tier 1 Risk] The agent wants to run: {}", tool_name);

        let delay_secs = self.config.security.confirm_tier1_delay;
        println!(
            "Auto-approving in {} seconds. Press ENTER to approve now...",
            delay_secs
        );

        let mut reader = BufReader::new(tokio::io::stdin());
        let mut buf = String::new();

        match tokio::time::timeout(Duration::from_secs(delay_secs), reader.read_line(&mut buf))
            .await
        {
            Ok(Ok(0)) => {
                debug!(task_id = %task_id, "stdin not available, auto-approving Tier1");
                println!("Auto-approved (no stdin).");
                "auto".to_string()
            }
            Ok(Ok(_)) => {
                println!("Approved by user.");
                "user".to_string()
            }
            Ok(Err(_)) => {
                debug!(task_id = %task_id, "stdin not available, auto-approving Tier1");
                "auto".to_string()
            }
            Err(_) => {
                println!("Auto-approved (timeout).");
                "countdown".to_string()
            }
        }
    }

    async fn confirm_tier2(&self, task_id: &str, tool_name: &str) -> Result<String> {
        if !self.config.security.require_explicit_tier2 {
            debug!(
                task_id = %task_id,
                tool = %tool_name,
                "Tier 2 operation auto-approved (require_explicit_tier2=false)"
            );
            return Ok("auto".to_string());
        }

        warn!(
            task_id = %task_id,
            tool = %tool_name,
            "Tier 2 operation: {} (destructive/irreversible)",
            tool_name
        );
        println!(
            "\n[Tier 2 Risk] The agent wants to run a potentially dangerous operation: {}",
            tool_name
        );
        println!("To approve, please type 'Y' and press ENTER. Any other input will abort.");

        let mut reader = BufReader::new(tokio::io::stdin());
        let mut buf = String::new();
        match reader.read_line(&mut buf).await {
            Ok(_)
                if buf.trim().eq_ignore_ascii_case("y")
                    || buf.trim().eq_ignore_ascii_case("yes") =>
            {
                println!("Approved by user.");
                Ok("user".to_string())
            }
            Ok(_) => Err(EngineError::OperationAbortedByUser.into()),
            Err(_) => {
                warn!(
                    task_id = %task_id,
                    "Tier 2 operation requires explicit approval but stdin not available"
                );
                Err(EngineError::OperationAbortedByUser.into())
            }
        }
    }
}
