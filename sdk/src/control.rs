use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthState {
    Uninitialized,
    Locked,
    Tampered,
    Unlocked,
    ReauthRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfo {
    pub access_token: String,
    pub expires_in_secs: u64,
    pub absolute_expires_in_secs: u64,
    pub reauth_required_for: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthStatus {
    pub state: AuthState,
    pub idle_expires_in_secs: Option<u64>,
    pub absolute_expires_in_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PasskeyStatus {
    #[serde(default)]
    pub supported: bool,
    #[serde(default)]
    pub registered: bool,
    #[serde(default)]
    pub credential_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PasskeyDescriptor {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub rp_id: String,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PasskeyRegistrationStartRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PasskeyChallengeResponse {
    pub challenge_id: String,
    pub options: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PasskeyFinishRequest {
    pub challenge_id: String,
    pub credential: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonHello {
    pub version: String,
    pub daemon_running: bool,
    pub auth_state: AuthState,
    pub node: NodeSummary,
    pub capabilities: DaemonCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeSummary {
    pub node_id: String,
    pub node_name: String,
    pub role: NodeExecutionRole,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DaemonCapabilities {
    pub brains: Vec<String>,
    pub services: Vec<String>,
    pub extensions: Vec<String>,
}

/// Public kind for installable Rove capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtensionKind {
    Skill,
    System,
    Connector,
    Channel,
}

impl ExtensionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::System => "system",
            Self::Connector => "connector",
            Self::Channel => "channel",
        }
    }
}

/// Public kind for optional daemon services.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceKind {
    Logging,
    WebUi,
    Remote,
    ConnectorEngine,
}

impl ServiceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Logging => "logging",
            Self::WebUi => "webui",
            Self::Remote => "remote",
            Self::ConnectorEngine => "connector-engine",
        }
    }
}

/// Public family for installed brains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrainFamily {
    Dispatch,
    Reasoning,
    Embedding,
    Rerank,
    Vision,
}

impl BrainFamily {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Dispatch => "dispatch",
            Self::Reasoning => "reasoning",
            Self::Embedding => "embedding",
            Self::Rerank => "rerank",
            Self::Vision => "vision",
        }
    }
}

/// Scope for policy files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyScope {
    User,
    Workspace,
    Project,
}

impl PolicyScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Workspace => "workspace",
            Self::Project => "project",
        }
    }
}

/// Top-level task execution mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunMode {
    Serial,
    Parallel,
}

/// Explicit workspace isolation mode for top-level tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunIsolation {
    None,
    Worktree,
    Snapshot,
}

/// Stable identifier for a top-level run context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunContextId(pub String);

/// Stable identity for a remote node in the mesh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeIdentity {
    pub node_id: String,
    pub node_name: String,
    pub public_key: String,
}

/// Execution profile for a remote node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeExecutionRole {
    Full,
    ExecutorOnly,
}

/// Capability profile advertised by a remote node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeProfile {
    pub capabilities: Vec<String>,
    pub tags: Vec<String>,
    pub execution_role: NodeExecutionRole,
}

/// Lightweight runtime load advertised by a remote node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct NodeLoadSnapshot {
    pub pending_tasks: u64,
    pub running_tasks: u64,
    pub recent_failures: u64,
    pub recent_successes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_load_percent: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_ram_mb: Option<u64>,
    pub recent_avg_duration_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedAgentEnvironment {
    pub id: String,
    pub profile_name: String,
    pub loadout_name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub builtins: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drivers: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brain_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_profile: Option<String>,
    #[serde(default)]
    pub active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedAgentSessionStatus {
    Ready,
    Running,
    Idle,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManagedAgentSession {
    pub id: String,
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    pub environment_id: String,
    pub profile_name: String,
    pub loadout_name: String,
    pub primary_thread_id: String,
    pub status: ManagedAgentSessionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_task_id: Option<String>,
    pub created_at: i64,
    pub last_active_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManagedAgentSessionEvent {
    pub position: i64,
    pub id: String,
    pub session_id: String,
    pub event_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    pub payload: Value,
    pub created_at: i64,
}

/// Reachability record for a remote transport path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RemoteTransportRecord {
    pub kind: String,
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_id: Option<String>,
    #[serde(default)]
    pub reachable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_checked_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Persisted discovery candidate for remote transports such as ZeroTier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RemoteDiscoveryCandidate {
    pub candidate_id: String,
    pub transport_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_id: Option<String>,
    pub member_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_name_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<NodeIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<NodeProfile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assigned_addresses: Vec<String>,
    pub last_seen_at: i64,
    #[serde(default)]
    pub controller_access: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paired_node_name: Option<String>,
    #[serde(default)]
    pub trusted: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transports: Vec<RemoteTransportRecord>,
}

/// Official remote transport plugin status surface for ZeroTier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ZeroTierStatus {
    pub enabled: bool,
    pub installed: bool,
    pub configured: bool,
    pub token_configured: bool,
    pub service_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_id: Option<String>,
    pub managed_name_sync: bool,
    pub service_online: bool,
    pub joined: bool,
    pub controller_access: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_status: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assigned_addresses: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transport_records: Vec<RemoteTransportRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_sync_at: Option<i64>,
    pub candidate_count: usize,
    pub sync_state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Official browser-control profile mode exposed across CLI, API, and WebUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BrowserProfileMode {
    #[default]
    ManagedLocal,
    AttachExisting,
    RemoteCdp,
}

impl BrowserProfileMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ManagedLocal => "managed_local",
            Self::AttachExisting => "attach_existing",
            Self::RemoteCdp => "remote_cdp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BrowserProfileReadiness {
    #[default]
    Ready,
    NeedsSetup,
    Warning,
}

impl BrowserProfileReadiness {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::NeedsSetup => "needs_setup",
            Self::Warning => "warning",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserApprovalControls {
    #[serde(default)]
    pub require_approval_for_managed_launch: bool,
    #[serde(default)]
    pub require_approval_for_existing_session_attach: bool,
    #[serde(default)]
    pub require_approval_for_remote_cdp: bool,
}

impl Default for BrowserApprovalControls {
    fn default() -> Self {
        Self {
            require_approval_for_managed_launch: true,
            require_approval_for_existing_session_attach: true,
            require_approval_for_remote_cdp: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BrowserProfileInput {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub mode: BrowserProfileMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_data_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cdp_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BrowserProfileRecord {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub mode: BrowserProfileMode,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_data_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cdp_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub readiness: BrowserProfileReadiness,
    #[serde(default)]
    pub approval_required: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BrowserSurfaceStatus {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_profile_id: Option<String>,
    #[serde(default)]
    pub controls: BrowserApprovalControls,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<BrowserProfileRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BrowserSurfaceUpdate {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_profile_id: Option<String>,
    #[serde(default)]
    pub controls: BrowserApprovalControls,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<BrowserProfileInput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VoiceEngineKind {
    #[default]
    NativeOs,
    LocalWhisper,
    LocalPiper,
}

impl VoiceEngineKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NativeOs => "native_os",
            Self::LocalWhisper => "local_whisper",
            Self::LocalPiper => "local_piper",
        }
    }

    pub fn supports_input(&self) -> bool {
        matches!(self, Self::NativeOs | Self::LocalWhisper)
    }

    pub fn supports_output(&self) -> bool {
        matches!(self, Self::NativeOs | Self::LocalPiper)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VoiceEngineReadiness {
    #[default]
    Ready,
    NeedsSetup,
    Warning,
    Unsupported,
}

impl VoiceEngineReadiness {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::NeedsSetup => "needs_setup",
            Self::Warning => "warning",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VoiceAssetStatus {
    #[default]
    NoneRequired,
    Managed,
    Missing,
    Ready,
}

impl VoiceAssetStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NoneRequired => "none_required",
            Self::Managed => "managed",
            Self::Missing => "missing",
            Self::Ready => "ready",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VoiceDeviceKind {
    #[default]
    Input,
    Output,
}

impl VoiceDeviceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoicePolicyControls {
    #[serde(default)]
    pub require_approval_for_tts: bool,
    #[serde(default)]
    pub require_approval_for_stt: bool,
    #[serde(default)]
    pub allow_remote_audio_input: bool,
    #[serde(default)]
    pub allow_remote_audio_output: bool,
    #[serde(default)]
    pub persist_transcripts: bool,
}

impl Default for VoicePolicyControls {
    fn default() -> Self {
        Self {
            require_approval_for_tts: true,
            require_approval_for_stt: true,
            allow_remote_audio_input: false,
            allow_remote_audio_output: false,
            persist_transcripts: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VoiceEngineInput {
    #[serde(default)]
    pub kind: VoiceEngineKind,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VoiceDeviceRecord {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: VoiceDeviceKind,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VoiceRuntimeStatus {
    pub system_id: String,
    #[serde(default)]
    pub installed: bool,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VoiceEngineRecord {
    #[serde(default)]
    pub kind: VoiceEngineKind,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub installed: bool,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub supports_input: bool,
    #[serde(default)]
    pub supports_output: bool,
    #[serde(default)]
    pub active_input: bool,
    #[serde(default)]
    pub active_output: bool,
    #[serde(default)]
    pub asset_status: VoiceAssetStatus,
    pub readiness: VoiceEngineReadiness,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default)]
    pub approval_required_for_input: bool,
    #[serde(default)]
    pub approval_required_for_output: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VoiceSurfaceStatus {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub runtime: VoiceRuntimeStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_input_engine: Option<VoiceEngineKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_output_engine: Option<VoiceEngineKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_input_device_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_output_device_id: Option<String>,
    #[serde(default)]
    pub policy: VoicePolicyControls,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub devices: Vec<VoiceDeviceRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub engines: Vec<VoiceEngineRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VoiceSurfaceUpdate {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_input_engine: Option<VoiceEngineKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_output_engine: Option<VoiceEngineKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_input_device_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_output_device_id: Option<String>,
    #[serde(default)]
    pub policy: VoicePolicyControls,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub engines: Vec<VoiceEngineInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VoiceEngineInstallRequest {
    #[serde(default)]
    pub engine: VoiceEngineKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VoiceEngineSelectionRequest {
    #[serde(default)]
    pub engine: VoiceEngineKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VoiceInputTestRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VoiceOutputTestRequest {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VoiceTestResult {
    #[serde(default)]
    pub ok: bool,
    pub engine: VoiceEngineKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionTrustBadge {
    Official,
    Verified,
    #[default]
    Unverified,
}

impl ExtensionTrustBadge {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Official => "official",
            Self::Verified => "verified",
            Self::Unverified => "unverified",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExtensionProvenance {
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry: Option<String>,
    #[serde(default)]
    pub catalog_managed: bool,
    #[serde(default)]
    pub advanced_source: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CatalogVersionRecord {
    pub version: String,
    pub published_at: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permission_summary: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permission_warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CatalogExtensionRecord {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub description: String,
    pub trust_badge: ExtensionTrustBadge,
    pub provenance: ExtensionProvenance,
    pub latest: CatalogVersionRecord,
    #[serde(default)]
    pub installed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_version: Option<String>,
    #[serde(default)]
    pub update_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExtensionUpdateRecord {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub installed_version: String,
    pub latest_version: String,
    pub trust_badge: ExtensionTrustBadge,
    pub provenance: ExtensionProvenance,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permission_summary: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permission_warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_summary: Option<String>,
}

/// Starter catalog group for official setup surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StarterCatalogKind {
    AgentTemplate,
    WorkflowTemplate,
    WorkerPreset,
    ChannelStarter,
    CapabilityPack,
}

impl StarterCatalogKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AgentTemplate => "agent_template",
            Self::WorkflowTemplate => "workflow_template",
            Self::WorkerPreset => "worker_preset",
            Self::ChannelStarter => "channel_starter",
            Self::CapabilityPack => "capability_pack",
        }
    }
}

impl Default for StarterCatalogKind {
    fn default() -> Self {
        Self::AgentTemplate
    }
}

/// Availability state for an official starter entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StarterCatalogStatus {
    Available,
    NeedsSetup,
    Ready,
}

impl StarterCatalogStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::NeedsSetup => "needs_setup",
            Self::Ready => "ready",
        }
    }
}

impl Default for StarterCatalogStatus {
    fn default() -> Self {
        Self::Available
    }
}

/// Unified official starter catalog entry exposed across CLI, API, and WebUI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StarterCatalogEntry {
    pub id: String,
    pub kind: StarterCatalogKind,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub official: bool,
    pub status: StarterCatalogStatus,
    pub source: String,
    pub action_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_route: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// Envelope sent between remote daemons for coordinated execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteEnvelope {
    pub origin_node: String,
    pub target_node: String,
    pub coordinator_node: String,
    pub task_id: String,
    pub task_input: String,
    pub stream_policy: String,
    pub execution_plan: Option<RemoteExecutionPlan>,
}

/// Coordinator-computed direct execution plan for executor-oriented remote nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteExecutionPlan {
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<RemoteExecutionStep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_args: Option<Value>,
    pub domain_hint: Option<String>,
}

/// One ordered execution step inside a remote direct-execution bundle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteExecutionStep {
    pub summary: String,
    pub tool_name: String,
    pub tool_args: Value,
}

impl RemoteExecutionPlan {
    pub fn direct(
        summary: impl Into<String>,
        tool_name: impl Into<String>,
        tool_args: Value,
        domain_hint: Option<String>,
    ) -> Self {
        let summary = summary.into();
        let tool_name = tool_name.into();
        let step = RemoteExecutionStep {
            summary: summary.clone(),
            tool_name: tool_name.clone(),
            tool_args: tool_args.clone(),
        };

        Self {
            summary,
            steps: vec![step],
            tool_name: Some(tool_name),
            tool_args: Some(tool_args),
            domain_hint,
        }
    }

    pub fn append_step(
        &mut self,
        summary: impl Into<String>,
        tool_name: impl Into<String>,
        tool_args: Value,
    ) {
        self.steps.push(RemoteExecutionStep {
            summary: summary.into(),
            tool_name: tool_name.into(),
            tool_args,
        });
    }

    pub fn steps(&self) -> Vec<RemoteExecutionStep> {
        if !self.steps.is_empty() {
            return self.steps.clone();
        }

        match self.tool_name.as_ref() {
            Some(tool_name) => vec![RemoteExecutionStep {
                summary: self.summary.clone(),
                tool_name: tool_name.clone(),
                tool_args: self.tool_args.clone().unwrap_or(Value::Null),
            }],
            None => Vec::new(),
        }
    }

    pub fn primary_tool_name(&self) -> Option<&str> {
        self.steps
            .first()
            .map(|step| step.tool_name.as_str())
            .or(self.tool_name.as_deref())
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty() && self.tool_name.is_none()
    }
}
