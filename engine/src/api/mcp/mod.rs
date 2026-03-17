//! Model Context Protocol (MCP) Integration
//!
//! This module provides secure integration with external MCP servers.
//! Every MCP server runs inside an OS-level sandbox (Gate 5) to prevent
//! compromised external processes from accessing the host system.
//!
//! Phase 4 — MCP + Gate 5 + WASM Plugins

pub mod sandbox;
pub mod spawner;

pub use sandbox::{McpSandbox, SandboxProfile};
pub use spawner::{McpServer, McpServerConfig, McpSpawner};
