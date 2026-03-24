use anyhow::Result;
use sha2::Digest;
use tracing::{debug, warn};

use crate::llm::{Message, ToolCall};
use crate::risk_assessor::{classify_terminal_command, Operation};
use crate::security::secrets::scrub_text;
use sdk::errors::EngineError;
use sdk::TaskSource;

use super::{AgentCore, MAX_RESULT_SIZE};

pub(super) struct ToolExecution {
    pub(super) safe_result: String,
}

impl AgentCore {
    pub(super) async fn run_policy_preflight(
        &mut self,
        task_id: &uuid::Uuid,
        domain: &str,
    ) -> Result<()> {
        let commands = self.policy_preflight_commands.clone();
        for command in commands {
            self.execute_scripted_command(task_id, 0, domain, &command)
                .await?;
        }
        Ok(())
    }

    pub(super) async fn run_policy_after_write(
        &mut self,
        task_id: &uuid::Uuid,
        iteration: usize,
        tool_name: &str,
        domain: &str,
    ) -> Result<()> {
        if tool_name != "write_file" {
            return Ok(());
        }

        let commands = self.policy_after_write_commands.clone();
        for command in commands {
            self.execute_scripted_command(task_id, iteration, domain, &command)
                .await?;
        }
        Ok(())
    }

    pub(super) async fn execute_scripted_command(
        &mut self,
        task_id: &uuid::Uuid,
        iteration: usize,
        domain: &str,
        command: &str,
    ) -> Result<()> {
        if !self.policy_executed_commands.insert(command.to_string()) {
            return Ok(());
        }

        let tool_call = ToolCall::new(
            format!("policy-{}-{}", task_id, self.policy_executed_commands.len()),
            "run_command",
            serde_json::json!({ "command": command }).to_string(),
        );

        self.memory
            .add_message(self.assistant_tool_message(task_id, &tool_call));
        self.insert_tool_call_event(task_id, &tool_call, iteration, domain)
            .await?;

        let execution = self
            .execute_tool_call(&task_id.to_string(), &tool_call)
            .await?;
        self.memory.add_message(crate::llm::Message::tool_result(
            &execution.safe_result,
            &tool_call.id,
        ));
        self.insert_observation_event(task_id, &execution.safe_result, iteration, domain)
            .await?;
        Ok(())
    }

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
        self.ensure_tool_allowed(&tool_call.name)?;
        let tool_args = self.parse_tool_arguments(task_id, tool_call);
        let tool_tier = self.audit_risk_tier(&tool_call.name, &tool_args)?;
        let approved_by = match tool_tier {
            0 => "auto".to_string(),
            _ => "registry".to_string(),
        };

        let tool_result = self
            .dispatch_tool(task_id, &tool_call.name, tool_args.clone())
            .await?;
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

        self.record_tool_audit(task_id, tool_call, tool_tier, &approved_by, &tool_result)
            .await;

        Ok(ToolExecution {
            safe_result: tool_result,
        })
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

    fn ensure_tool_allowed(&self, tool_name: &str) -> Result<()> {
        let Some(profile) = self.current_execution_profile.as_ref() else {
            return Ok(());
        };
        if profile.allowed_tools.is_empty() {
            return Ok(());
        }
        if profile.allowed_tools.iter().any(|allowed| allowed == tool_name) {
            return Ok(());
        }
        anyhow::bail!(
            "tool '{}' is not allowed for agent '{}'",
            tool_name,
            profile.agent_name.as_deref().unwrap_or("unknown")
        )
    }

    async fn dispatch_tool(
        &self,
        task_id: &str,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<String> {
        if tool_name == "write_file" {
            if let Some(workspace) = self.tools.workspace() {
                let lock = self.workspace_locks.get_lock(workspace);
                let _guard = lock.lock().await;
                return match self
                    .tools
                    .call(tool_name, args, task_id, &self.current_task_source())
                    .await
                {
                    Ok(result) => Ok(stringify_tool_result(result)),
                    Err(error) => map_tool_error(error),
                };
            }
        }

        match self
            .tools
            .call(tool_name, args, task_id, &self.current_task_source())
            .await
        {
            Ok(result) => Ok(stringify_tool_result(result)),
            Err(error) => map_tool_error(error),
        }
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

    fn audit_risk_tier(&self, tool_name: &str, args: &serde_json::Value) -> Result<i32> {
        let op_name = self.tool_operation_name(tool_name, args);

        let arg_strings = match args {
            serde_json::Value::Object(map) => map
                .values()
                .filter_map(|value| value.as_str().map(ToOwned::to_owned))
                .collect(),
            _ => Vec::new(),
        };

        let operation = Operation::new(op_name, arg_strings, self.current_source.clone());
        let tier = self.risk_assessor.assess(&operation)?;

        let tier_value = match tier {
            crate::risk_assessor::RiskTier::Tier0 => 0,
            crate::risk_assessor::RiskTier::Tier1 => 1,
            crate::risk_assessor::RiskTier::Tier2 => 2,
        };

        debug!(tool = %tool_name, "Tool '{}' assessed as audit tier {}", tool_name, tier_value);
        Ok(tier_value)
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

    fn current_task_source(&self) -> TaskSource {
        match self.current_source {
            crate::risk_assessor::OperationSource::Local => TaskSource::Cli,
            crate::risk_assessor::OperationSource::Remote => {
                TaskSource::Remote("runtime".to_string())
            }
        }
    }
}

fn stringify_tool_result(result: serde_json::Value) -> String {
    match result {
        serde_json::Value::String(text) => text,
        other => other.to_string(),
    }
}

fn map_tool_error(error: EngineError) -> Result<String> {
    match error {
        EngineError::OperationAbortedByUser => Err(error.into()),
        other => Ok(format!("ERROR: {}", other)),
    }
}
