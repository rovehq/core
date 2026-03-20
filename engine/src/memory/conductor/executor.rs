//! Conductor Executor
//!
//! Executes individual `PlanStep`s by dispatching to the appropriate tools
//! (filesystem, terminal, vision) based on the step type and LLM guidance.

use crate::conductor::policy::StepExecutionPolicy;
use crate::conductor::types::{PlanStep, StepResult, StepType};

use crate::builtin_tools::FilesystemTool;
use crate::builtin_tools::TerminalTool;
use crate::llm::router::LLMRouter;
use crate::llm::{LLMResponse, Message};
use anyhow::Result;
use sdk::Route;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

/// Executes individual plan steps using available tools
pub struct Executor {
    router: Arc<LLMRouter>,
    fs_tool: Option<Arc<FilesystemTool>>,
    terminal_tool: Option<Arc<TerminalTool>>,
}

impl Executor {
    pub fn new(
        router: Arc<LLMRouter>,
        fs_tool: Option<Arc<FilesystemTool>>,
        terminal_tool: Option<Arc<TerminalTool>>,
    ) -> Self {
        Self {
            router,
            fs_tool,
            terminal_tool,
        }
    }

    /// Execute a single plan step
    ///
    /// The executor:
    /// 1. Asks the LLM what tool calls are needed for this step
    /// 2. Dispatches tool calls to the appropriate tool
    /// 3. Feeds results back to the LLM for next action
    /// 4. Repeats until LLM produces a final answer or max iterations hit
    /// 5. Returns a StepResult summarizing what happened
    pub async fn execute_step(&self, step: &PlanStep, context: &str) -> Result<StepResult> {
        let start = Instant::now();
        let mut tools_used = Vec::new();
        let mut logs = String::new();
        let mut context_extracted = String::new();
        let policy = StepExecutionPolicy::for_step(step, Route::Ollama);

        // Build messages for the LLM with step context
        let system = Message::system(format!(
            "{}\n\n\
            Available tools:\n\
            - read_file(path): Read a file's contents\n\
            - write_file(path, content): Write content to a file\n\
            - execute_command(command): Run a shell command\n\n\
            When you have completed the step, provide your final answer summarizing what was done and any important findings.",
            policy.system_prompt(step, context)
        ));

        let user_msg = Message::user(&step.description);
        let mut messages = vec![system, user_msg];

        const MAX_TOOL_ITERATIONS: usize = 10;

        for iteration in 0..MAX_TOOL_ITERATIONS {
            debug!(
                "Step {} iteration {}/{}",
                step.id,
                iteration + 1,
                MAX_TOOL_ITERATIONS
            );

            let response = self.router.call(&messages).await;

            match response {
                Ok((LLMResponse::ToolCall(tool_call), _provider)) => {
                    info!("Step {} calling tool: {}", step.id, tool_call.name);
                    tools_used.push(tool_call.name.clone());

                    let tool_result = self
                        .dispatch_tool(&tool_call.name, &tool_call.arguments)
                        .await;

                    let result_text = match tool_result {
                        Ok(output) => {
                            logs.push_str(&format!(
                                "[{}] {} -> OK ({} bytes)\n",
                                tool_call.name,
                                tool_call.arguments,
                                output.len()
                            ));
                            output
                        }
                        Err(e) => {
                            let err = format!("error: {}", e);
                            logs.push_str(&format!(
                                "[{}] {} -> FAIL: {}\n",
                                tool_call.name, tool_call.arguments, e
                            ));
                            err
                        }
                    };

                    // Feed result back to LLM
                    messages.push(Message::assistant(format!(
                        "Called tool: {}({})",
                        tool_call.name, tool_call.arguments
                    )));
                    messages.push(Message::tool_result(&result_text, &tool_call.id));
                }
                Ok((LLMResponse::FinalAnswer(answer), _provider)) => {
                    info!(
                        "Step {} completed in {:.1}s with {} tool calls",
                        step.id,
                        start.elapsed().as_secs_f64(),
                        tools_used.len()
                    );
                    context_extracted = answer.content;
                    break;
                }
                Err(e) => {
                    warn!("Step {} LLM call failed: {}", step.id, e);
                    logs.push_str(&format!("error: LLM call failed: {}\n", e));
                    return Ok(StepResult::Failed(format!(
                        "Step {} LLM call failed: {}",
                        step.id, e
                    )));
                }
            }
        }

        // Determine success based on step type
        let success = match step.step_type {
            StepType::Research => !context_extracted.is_empty(),
            StepType::Execute => !logs.contains("error:") && !logs.contains("FAIL:"),
            StepType::Verify => {
                !logs.contains("error:") && !logs.contains("FAIL:") && !context_extracted.is_empty()
            }
        };

        if success {
            Ok(StepResult::Success)
        } else {
            Ok(StepResult::Failed(logs))
        }
    }

    /// Dispatch a tool call to the appropriate tool implementation
    async fn dispatch_tool(&self, tool_name: &str, arguments: &str) -> Result<String> {
        // Parse arguments as JSON
        let args: serde_json::Value = serde_json::from_str(arguments)
            .unwrap_or_else(|_| serde_json::json!({"input": arguments}));

        match tool_name {
            "read_file" => {
                let path = args
                    .get("path")
                    .and_then(|p| p.as_str())
                    .ok_or_else(|| anyhow::anyhow!("read_file requires 'path' argument"))?;

                match &self.fs_tool {
                    Some(fs) => fs.read_file(path).await,
                    None => Err(anyhow::anyhow!("Filesystem tool not available")),
                }
            }
            "write_file" => {
                let path = args
                    .get("path")
                    .and_then(|p| p.as_str())
                    .ok_or_else(|| anyhow::anyhow!("write_file requires 'path' argument"))?;
                let content = args
                    .get("content")
                    .and_then(|c| c.as_str())
                    .ok_or_else(|| anyhow::anyhow!("write_file requires 'content' argument"))?;

                match &self.fs_tool {
                    Some(fs) => fs.write_file(path, content).await,
                    None => Err(anyhow::anyhow!("Filesystem tool not available")),
                }
            }
            "execute_command" => {
                let command = args
                    .get("command")
                    .and_then(|c| c.as_str())
                    .ok_or_else(|| {
                        anyhow::anyhow!("execute_command requires 'command' argument")
                    })?;

                match &self.terminal_tool {
                    Some(term) => term.execute(command).await,
                    None => Err(anyhow::anyhow!("Terminal tool not available")),
                }
            }
            _ => Err(anyhow::anyhow!("Unknown tool: {}", tool_name)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conductor::types::RoutePolicy;

    fn make_step(step_type: StepType) -> PlanStep {
        let role = crate::conductor::types::StepRole::for_step_type(&step_type);
        let parallel_safe = matches!(&step_type, StepType::Research | StepType::Verify);
        PlanStep {
            id: "test_step".to_string(),
            order: 0,
            description: "test step".to_string(),
            step_type,
            role,
            parallel_safe,
            route_policy: RoutePolicy::Inherit,
            dependencies: vec![],
            expected_outcome: "done".to_string(),
        }
    }

    #[test]
    fn test_executor_creation() {
        use crate::config::LLMConfig;
        use crate::llm::router::LLMRouter;

        let config = Arc::new(LLMConfig {
            default_provider: "ollama".to_string(),
            sensitivity_threshold: 0.7,
            complexity_threshold: 0.8,
            ollama: Default::default(),
            openai: Default::default(),
            anthropic: Default::default(),
            gemini: Default::default(),
            nvidia_nim: Default::default(),
            custom_providers: vec![],
        });

        let router = Arc::new(LLMRouter::new(vec![], config));
        let executor = Executor::new(router, None, None);

        // Executor should be constructable without tools
        let _ = executor;
    }

    #[test]
    fn test_step_types() {
        let research = make_step(StepType::Research);
        assert_eq!(research.step_type, StepType::Research);

        let execute = make_step(StepType::Execute);
        assert_eq!(execute.step_type, StepType::Execute);

        let verify = make_step(StepType::Verify);
        assert_eq!(verify.step_type, StepType::Verify);
    }
}
