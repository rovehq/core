//! Rove SDK
//!
//! Shared library providing traits, types, and utilities for Rove components.
//! This crate is used by both the engine and plugins/core-tools.

pub mod agent_handle;
pub mod agent_spec;
/// Browser backend trait and standard tool names
pub mod browser;
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
pub use agent_spec::{
    AgentFactoryResult, AgentRunRecord, AgentSpec, AgentTemplate, AgentUiSchema, CapabilityRef,
    ChannelBinding, FactoryFieldChange, FactoryReview, FileWatchBinding, NodePlacementPolicy,
    SpecProvenance, SpecRunStatus, TaskExecutionProfile, WebhookBinding, WorkerPreset,
    WorkflowBranchSpec, WorkflowFactoryResult, WorkflowRunDetail, WorkflowRunRecord,
    WorkflowRunStepRecord, WorkflowSpec, WorkflowStepSpec,
};
pub use brain::{
    Brain, BrainResponse, Complexity, DispatchResult, Message, Route, TaskDomain, ToolSchema,
    ToolTag,
};
pub use browser::BrowserBackend;
pub use bus_handle::{BusHandle, BusHandleImpl};
pub use config_handle::{
    ApprovalConfigSnapshot, ChannelsConfigSnapshot, ConfigHandle, ConfigHandleImpl,
    ConfigMetadataSnapshot, CoreConfigSnapshot, DaemonConfigSnapshot, LlmConfigSnapshot,
    SecretConfigSnapshot, ServicesConfigSnapshot, StaticConfigHandle, VersionedConfigSnapshot,
};
pub use control::{
    AuthState, AuthStatus, BrainFamily, BrowserApprovalControls, BrowserProfileInput,
    BrowserProfileMode, BrowserProfileReadiness, BrowserProfileRecord, BrowserSurfaceStatus,
    BrowserSurfaceUpdate, CatalogExtensionRecord, CatalogVersionRecord, DaemonCapabilities,
    DaemonHello, ExtensionKind, ExtensionProvenance, ExtensionTrustBadge, ExtensionUpdateRecord,
    NodeExecutionRole, NodeIdentity, NodeLoadSnapshot, NodeProfile, NodeSummary,
    PasskeyChallengeResponse, PasskeyDescriptor, PasskeyFinishRequest,
    PasskeyRegistrationStartRequest, PasskeyStatus, PolicyScope, RemoteDiscoveryCandidate,
    RemoteEnvelope, RemoteExecutionPlan, RemoteTransportRecord, RunContextId, RunIsolation,
    RunMode, ServiceKind, SessionInfo, StarterCatalogEntry, StarterCatalogKind,
    StarterCatalogStatus, VoiceAssetStatus, VoiceDeviceKind, VoiceDeviceRecord,
    VoiceEngineInput, VoiceEngineInstallRequest, VoiceEngineKind, VoiceEngineReadiness,
    VoiceEngineRecord, VoiceEngineSelectionRequest, VoiceInputTestRequest,
    VoiceOutputTestRequest, VoicePolicyControls, VoiceRuntimeStatus, VoiceSurfaceStatus,
    VoiceSurfaceUpdate, VoiceTestResult, ZeroTierStatus,
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
