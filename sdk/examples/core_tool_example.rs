//! Example demonstrating CoreTool trait and CoreContext usage
//!
//! This example shows how to implement a core tool that uses CoreContext
//! to interact with the engine.

use sdk::{
    AgentHandle, AgentHandleImpl, BusHandle, BusHandleImpl, ConfigHandle, ConfigHandleImpl,
    CoreContext, CoreTool, CryptoHandle, CryptoHandleImpl, DbHandle, DbHandleImpl, EngineError,
    NetworkHandle, NetworkHandleImpl, ToolInput, ToolOutput,
};
use serde_json::json;
use std::sync::Arc;

/// Example core tool that demonstrates CoreContext usage
struct ExampleTool {
    ctx: Option<CoreContext>,
}

impl ExampleTool {
    fn new() -> Self {
        Self { ctx: None }
    }
}

impl CoreTool for ExampleTool {
    fn name(&self) -> &str {
        "example-tool"
    }

    fn version(&self) -> &str {
        "0.1.0"
    }

    fn start(&mut self, ctx: CoreContext) -> Result<(), EngineError> {
        println!("ExampleTool starting...");

        // Store the context for later use
        self.ctx = Some(ctx.clone());

        // Example: Read configuration
        if let Some(workspace) = ctx.config.get_string("core.workspace") {
            println!("Workspace configured: {}", workspace);
        }

        // Example: Subscribe to events
        ctx.bus.subscribe("TaskCompleted")?;
        println!("Subscribed to TaskCompleted events");

        Ok(())
    }

    fn stop(&mut self) -> Result<(), EngineError> {
        println!("ExampleTool stopping...");
        self.ctx = None;
        Ok(())
    }

    fn handle(&self, input: ToolInput) -> Result<ToolOutput, EngineError> {
        let ctx = self
            .ctx
            .as_ref()
            .ok_or_else(|| EngineError::Config("Tool not initialized".to_string()))?;

        match input.method.as_str() {
            "submit_task" => {
                // Example: Submit a task to the agent
                let task_input = input
                    .param_str("task")
                    .map_err(|e| EngineError::Config(e.to_string()))?;
                let task_id = ctx.agent.submit_task(task_input)?;
                Ok(ToolOutput::json(json!({ "task_id": task_id })))
            }

            "query_history" => {
                // Example: Query database for task history
                let limit = input.param_i64_opt("limit").unwrap_or(10);
                let results = ctx.db.query(
                    "SELECT id, input, status FROM tasks ORDER BY created_at DESC LIMIT ?",
                    vec![json!(limit)],
                )?;
                Ok(ToolOutput::json(json!({ "tasks": results })))
            }

            "get_config" => {
                // Example: Read configuration value
                let key = input
                    .param_str("key")
                    .map_err(|e| EngineError::Config(e.to_string()))?;
                match ctx.config.get(&key) {
                    Some(value) => Ok(ToolOutput::json(json!({ "value": value }))),
                    None => Ok(ToolOutput::error(format!("Config key not found: {}", key))),
                }
            }

            "sign_data" => {
                // Example: Sign data using crypto handle
                let data = input
                    .param_str("data")
                    .map_err(|e| EngineError::Config(e.to_string()))?;
                let signature = ctx.crypto.sign_data(data.as_bytes())?;
                let sig_hex = hex::encode(signature);
                Ok(ToolOutput::json(json!({ "signature": sig_hex })))
            }

            "http_get" => {
                // Example: Make HTTP request
                let url = input
                    .param_str("url")
                    .map_err(|e| EngineError::Config(e.to_string()))?;
                let response = ctx.network.http_get(&url)?;
                let response_text = String::from_utf8_lossy(&response).to_string();
                Ok(ToolOutput::text(response_text))
            }

            "publish_event" => {
                // Example: Publish event to message bus
                let event_type = input
                    .param_str("event_type")
                    .map_err(|e| EngineError::Config(e.to_string()))?;
                let payload = input.params.get("payload").cloned().unwrap_or(json!({}));
                ctx.bus.publish(&event_type, payload)?;
                Ok(ToolOutput::text("Event published"))
            }

            _ => Ok(ToolOutput::error(format!(
                "Unknown method: {}",
                input.method
            ))),
        }
    }
}

// Mock implementations for demonstration purposes
struct MockAgentHandle;
impl AgentHandleImpl for MockAgentHandle {
    fn submit_task(&self, task_input: String) -> Result<String, EngineError> {
        println!("Mock: Submitting task: {}", task_input);
        Ok("task-123".to_string())
    }

    fn get_task_status(&self, task_id: &str) -> Result<String, EngineError> {
        println!("Mock: Getting status for task: {}", task_id);
        Ok("completed".to_string())
    }
}

struct MockDbHandle;
impl DbHandleImpl for MockDbHandle {
    fn query(
        &self,
        sql: &str,
        _params: Vec<serde_json::Value>,
    ) -> Result<Vec<serde_json::Value>, EngineError> {
        println!("Mock: Executing query: {}", sql);
        Ok(vec![
            json!({"id": "task-1", "input": "test task 1", "status": "completed"}),
            json!({"id": "task-2", "input": "test task 2", "status": "running"}),
        ])
    }
}

struct MockConfigHandle;
impl ConfigHandleImpl for MockConfigHandle {
    fn get(&self, key: &str) -> Option<serde_json::Value> {
        println!("Mock: Getting config: {}", key);
        match key {
            "core.workspace" => Some(json!("~/projects")),
            "llm.default_provider" => Some(json!("ollama")),
            _ => None,
        }
    }
}

struct MockCryptoHandle;
impl CryptoHandleImpl for MockCryptoHandle {
    fn sign_data(&self, data: &[u8]) -> Result<Vec<u8>, EngineError> {
        println!("Mock: Signing {} bytes", data.len());
        Ok(vec![0xDE, 0xAD, 0xBE, 0xEF]) // Mock signature
    }

    fn verify_signature(&self, _data: &[u8], _signature: &[u8]) -> Result<(), EngineError> {
        println!("Mock: Verifying signature");
        Ok(())
    }

    fn get_secret(&self, key: &str) -> Result<String, EngineError> {
        println!("Mock: Getting secret: {}", key);
        Ok("mock-secret-value".to_string())
    }

    fn scrub_secrets(&self, text: &str) -> String {
        println!("Mock: Scrubbing secrets from text");
        text.replace("secret", "[REDACTED]")
    }
}

struct MockNetworkHandle;
impl NetworkHandleImpl for MockNetworkHandle {
    fn http_get(&self, url: &str) -> Result<Vec<u8>, EngineError> {
        println!("Mock: HTTP GET: {}", url);
        Ok(b"Mock response data".to_vec())
    }

    fn http_post(&self, url: &str, _body: Vec<u8>) -> Result<Vec<u8>, EngineError> {
        println!("Mock: HTTP POST: {}", url);
        Ok(b"Mock response data".to_vec())
    }
}

struct MockBusHandle;
impl BusHandleImpl for MockBusHandle {
    fn subscribe(&self, event_type: &str) -> Result<(), EngineError> {
        println!("Mock: Subscribing to: {}", event_type);
        Ok(())
    }

    fn publish(&self, event_type: &str, payload: serde_json::Value) -> Result<(), EngineError> {
        println!("Mock: Publishing {} event: {:?}", event_type, payload);
        Ok(())
    }
}

fn main() {
    println!("=== CoreTool and CoreContext Example ===\n");

    // Create mock handles
    let agent = AgentHandle::new(Arc::new(MockAgentHandle));
    let db = DbHandle::new(Arc::new(MockDbHandle));
    let config = ConfigHandle::new(Arc::new(MockConfigHandle));
    let crypto = CryptoHandle::new(Arc::new(MockCryptoHandle));
    let network = NetworkHandle::new(Arc::new(MockNetworkHandle));
    let bus = BusHandle::new(Arc::new(MockBusHandle));

    // Create CoreContext
    let ctx = CoreContext::new(agent, db, config, crypto, network, bus);

    // Create and initialize tool
    let mut tool = ExampleTool::new();

    println!("Tool name: {}", tool.name());
    println!("Tool version: {}\n", tool.version());

    // Start the tool
    match tool.start(ctx) {
        Ok(_) => println!("Tool started successfully\n"),
        Err(e) => println!("Error starting tool: {:?}\n", e),
    }

    // Example 1: Submit a task
    println!("--- Example 1: Submit Task ---");
    let input = ToolInput::new("submit_task").with_param("task", json!("Analyze the codebase"));
    match tool.handle(input) {
        Ok(output) => println!("Result: {}\n", output.to_json()),
        Err(e) => println!("Error: {:?}\n", e),
    }

    // Example 2: Query history
    println!("--- Example 2: Query History ---");
    let input = ToolInput::new("query_history").with_param("limit", json!(5));
    match tool.handle(input) {
        Ok(output) => println!("Result: {}\n", output.to_json()),
        Err(e) => println!("Error: {:?}\n", e),
    }

    // Example 3: Get config
    println!("--- Example 3: Get Config ---");
    let input = ToolInput::new("get_config").with_param("key", json!("core.workspace"));
    match tool.handle(input) {
        Ok(output) => println!("Result: {}\n", output.to_json()),
        Err(e) => println!("Error: {:?}\n", e),
    }

    // Example 4: Sign data
    println!("--- Example 4: Sign Data ---");
    let input = ToolInput::new("sign_data").with_param("data", json!("Important message"));
    match tool.handle(input) {
        Ok(output) => println!("Result: {}\n", output.to_json()),
        Err(e) => println!("Error: {:?}\n", e),
    }

    // Example 5: HTTP GET
    println!("--- Example 5: HTTP GET ---");
    let input = ToolInput::new("http_get").with_param("url", json!("https://api.example.com/data"));
    match tool.handle(input) {
        Ok(output) => println!("Result: {}\n", output.to_json()),
        Err(e) => println!("Error: {:?}\n", e),
    }

    // Example 6: Publish event
    println!("--- Example 6: Publish Event ---");
    let input = ToolInput::new("publish_event")
        .with_param("event_type", json!("CustomEvent"))
        .with_param("payload", json!({"message": "Hello from tool"}));
    match tool.handle(input) {
        Ok(output) => println!("Result: {}\n", output.to_json()),
        Err(e) => println!("Error: {:?}\n", e),
    }

    // Stop the tool
    match tool.stop() {
        Ok(_) => println!("\nTool stopped successfully"),
        Err(e) => println!("\nError stopping tool: {:?}", e),
    }
}
