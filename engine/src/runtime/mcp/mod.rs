//! runtime/mcp — Rove connecting to external MCP servers
//!
//! This module owns the inward MCP runtime: sandboxing, spawning, lifecycle,
//! discovery, and tool calls for external MCP servers that Rove connects to.

pub mod sandbox;
pub mod spawner;

pub use sandbox::{McpSandbox, SandboxProfile};
pub use spawner::{McpServer, McpServerConfig, McpSpawner, McpToolDescriptor};
