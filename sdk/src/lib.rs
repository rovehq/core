//! Rove SDK
//!
//! Shared library providing traits, types, and utilities for Rove components.
//! This crate is used by both the engine and plugins/core-tools.

pub mod agent_handle;
pub mod bus_handle;
pub mod config_handle;
pub mod control;
/// Core tool trait and context
pub mod core_tool;
pub mod crypto_handle;
pub mod db_handle;
pub mod network_handle;

/// Error types and handling
pub mod errors;

pub mod task;
pub mod tool_io;
/// Tool input/output compatibility re-exports
pub mod types;

/// Manifest types and compatibility re-exports
pub mod manifest;
pub mod permission;
pub mod plugin;
pub mod subagent;

/// Brain system types and traits
pub mod brain;

// Re-export commonly used types
pub use agent_handle::{AgentHandle, AgentHandleImpl};
pub use brain::{
    Brain, BrainResponse, Complexity, DispatchResult, Message, Route, TaskDomain, ToolSchema,
    ToolTag,
};
pub use bus_handle::{BusHandle, BusHandleImpl};
pub use config_handle::{ConfigHandle, ConfigHandleImpl};
pub use control::{
    BrainFamily, ExtensionKind, NodeExecutionRole, NodeIdentity, NodeProfile, PolicyScope,
    RemoteEnvelope, RemoteExecutionPlan, RunContextId, RunIsolation, RunMode, ServiceKind,
};
pub use core_tool::{CoreContext, CoreTool};
pub use crypto_handle::{CryptoHandle, CryptoHandleImpl};
pub use db_handle::{DbHandle, DbHandleImpl};
pub use errors::{EngineError, RoveErrorExt};
pub use manifest::{CoreToolEntry, Manifest, PluginEntry, PluginPermissions};
pub use network_handle::{NetworkHandle, NetworkHandleImpl};
pub use subagent::{SubagentRole, SubagentSpec};
pub use task::TaskSource;
pub use tool_io::{ToolError, ToolInput, ToolOutput};
