//! api/mcp — Rove as an MCP server
//!
//! This module is a compatibility surface for code that still refers to
//! `api::mcp`. The inward MCP runtime now lives under `runtime/mcp/`.
//!
//! OUTWARD direction:
//! external MCP-aware clients -> Rove
//!
//! INWARD direction:
//! Rove -> external MCP servers
//!
//! For the inward runtime, sandboxing, spawning, and lifecycle handling,
//! see `runtime/mcp/`.

pub mod sandbox;
pub mod spawner;

pub use sandbox::{McpSandbox, SandboxProfile};
pub use spawner::{McpServer, McpServerConfig, McpSpawner, McpToolDescriptor};
