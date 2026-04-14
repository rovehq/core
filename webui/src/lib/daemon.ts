'use client';

export type AuthState = 'uninitialized' | 'locked' | 'tampered' | 'unlocked' | 'reauth_required';
export type NodeRole = 'full' | 'executor_only';

export interface DaemonHello {
  version: string;
  daemon_running: boolean;
  auth_state: AuthState;
  node: {
    node_id: string;
    node_name: string;
    role: NodeRole;
  };
  capabilities: {
    brains: string[];
    services: string[];
    extensions: string[];
  };
}

export interface SessionInfo {
  access_token: string;
  expires_in_secs: number;
  absolute_expires_in_secs: number;
  reauth_required_for: string[];
}

export interface AuthStatus {
  state: AuthState;
  idle_expires_in_secs?: number | null;
  absolute_expires_in_secs?: number | null;
}

export interface TaskSummary {
  id: string;
  input: string;
  status: 'pending' | 'running' | 'completed' | 'failed';
  provider_used?: string | null;
  duration_ms?: number | null;
  created_at: number;
  completed_at?: number | null;
}

export interface ServiceStatus {
  name: string;
  enabled: boolean;
  details: Record<string, string>;
}

export interface ServiceInstallState {
  mode: 'login' | 'boot';
  installed: boolean;
  supported: boolean;
  path: string;
  label: string;
  default_profile: 'desktop' | 'headless';
  auto_restart: boolean;
}

export interface ServiceInstallStatus {
  current_binary?: string | null;
  default_port: number;
  login: ServiceInstallState;
  boot: ServiceInstallState;
}

export interface PathHealthStatus {
  path: string;
  exists: boolean;
  writable: boolean;
}

export interface HealthCheckRecord {
  name: string;
  ok: boolean;
  detail: string;
}

export interface AuthHealthSummary {
  password_state: string;
  session_state?: string | null;
  idle_expires_in_secs?: number | null;
  absolute_expires_in_secs?: number | null;
}

export interface ControlPlaneSummary {
  webui_enabled: boolean;
  configured_bind_addr: string;
  listen_addr: string;
  port: number;
  control_url: string;
  tls_enabled: boolean;
  current_binary?: string | null;
}

export interface TransportHealthSummary {
  name: string;
  enabled: boolean;
  configured: boolean;
  healthy: boolean;
  summary: string;
}

export interface RuntimeHealthSnapshot {
  healthy: boolean;
  initialized: boolean;
  config_file: PathHealthStatus;
  workspace: PathHealthStatus;
  data_dir: PathHealthStatus;
  database: PathHealthStatus;
  log_file: PathHealthStatus;
  policy_dir: PathHealthStatus;
  node_name: string;
  profile: string;
  secret_backend: string;
  daemon_running: boolean;
  daemon_pid?: number | null;
  auth: AuthHealthSummary;
  control_plane: ControlPlaneSummary;
  service_install: ServiceInstallStatus;
  services: ServiceStatus[];
  channels: ChannelStatus[];
  transports: TransportHealthSummary[];
  remote?: {
    enabled: boolean;
    node_name: string;
    paired_nodes: number;
    transport_count: number;
  } | null;
  checks: HealthCheckRecord[];
  issues: string[];
}

export interface NodeLoadSnapshot {
  pending_tasks: number;
  running_tasks: number;
  recent_failures: number;
  recent_successes: number;
  recent_avg_duration_ms?: number | null;
}

export interface RemoteStatus {
  enabled: boolean;
  node: {
    node_id: string;
    node_name: string;
    public_key: string;
  };
  profile: {
    capabilities: string[];
    tags: string[];
    execution_role: NodeRole;
  };
  paired_nodes: number;
  load?: NodeLoadSnapshot | null;
  transports: RemoteTransportRecord[];
}

export interface RemotePeer {
  identity: {
    node_id: string;
    node_name: string;
    public_key: string;
  };
  profile: {
    capabilities: string[];
    tags: string[];
    execution_role: NodeRole;
  };
  target: string;
  trusted: boolean;
  transports: RemoteTransportRecord[];
}

export interface RemoteTransportRecord {
  kind: string;
  address: string;
  base_url?: string | null;
  network_id?: string | null;
  reachable: boolean;
  latency_ms?: number | null;
  last_checked_at?: number | null;
  last_error?: string | null;
}

export interface RemoteDiscoveryCandidate {
  candidate_id: string;
  transport_kind: string;
  network_id?: string | null;
  member_id: string;
  member_name?: string | null;
  node_name_hint?: string | null;
  identity?: {
    node_id: string;
    node_name: string;
    public_key: string;
  } | null;
  profile?: {
    capabilities: string[];
    tags: string[];
    execution_role: NodeRole;
  } | null;
  assigned_addresses: string[];
  last_seen_at: number;
  controller_access: boolean;
  paired_node_name?: string | null;
  trusted: boolean;
  transports: RemoteTransportRecord[];
}

export interface ExtensionRecord {
  id: string;
  name: string;
  kind: string;
  state: string;
  source: string;
  description: string;
  version?: string | null;
  official: boolean;
  trust_badge: 'official' | 'verified' | 'unverified';
  provenance: ExtensionProvenance;
  latest_version?: string | null;
  update_available: boolean;
  permission_summary: string[];
  permission_warnings: string[];
  release_summary?: string | null;
}

export interface ExtensionProvenance {
  source: string;
  registry?: string | null;
  catalog_managed: boolean;
  advanced_source: boolean;
}

export interface CatalogVersionRecord {
  version: string;
  published_at: number;
  permission_summary: string[];
  permission_warnings: string[];
  release_summary?: string | null;
}

export interface CatalogExtensionRecord {
  id: string;
  name: string;
  kind: string;
  description: string;
  trust_badge: 'official' | 'verified' | 'unverified';
  provenance: ExtensionProvenance;
  latest: CatalogVersionRecord;
  installed: boolean;
  installed_version?: string | null;
  update_available: boolean;
}

export interface ExtensionUpdateRecord {
  id: string;
  name: string;
  kind: string;
  installed_version: string;
  latest_version: string;
  trust_badge: 'official' | 'verified' | 'unverified';
  provenance: ExtensionProvenance;
  permission_summary: string[];
  permission_warnings: string[];
  release_summary?: string | null;
}

export interface DaemonConfig {
  node_name: string;
  profile: 'desktop' | 'headless';
  developer_mode: boolean;
  privacy_mode: string;
  idle_timeout_secs: number;
  absolute_timeout_secs: number;
  reauth_window_secs: number;
  session_persist_on_restart: boolean;
  approval_mode: 'default' | 'allowlist' | 'open' | 'assisted';
  approvals_rules_path: string;
  secret_backend: 'auto' | 'vault' | 'keychain' | 'env';
  bind_addr: string;
  tls_enabled: boolean;
  tls_cert_path: string;
  tls_key_path: string;
}

export interface CapabilityRef {
  kind: string;
  name: string;
  required: boolean;
}

export interface ChannelBinding {
  kind: string;
  target?: string | null;
  enabled: boolean;
  provenance?: SpecProvenance | null;
}

export interface NodePlacementPolicy {
  preferred_nodes: string[];
  required_tags: string[];
  allow_local: boolean;
  require_executor: boolean;
}

export interface AgentUiSchema {
  icon?: string | null;
  accent?: string | null;
}

export interface AgentSpec {
  schema_version: number;
  id: string;
  name: string;
  purpose: string;
  instructions: string;
  enabled: boolean;
  capabilities: CapabilityRef[];
  channels: ChannelBinding[];
  model_policy?: string | null;
  memory_policy: string;
  approval_mode?: string | null;
  runtime_profile?: string | null;
  node_placement: NodePlacementPolicy;
  schedules: string[];
  output_contract?: string | null;
  ui: AgentUiSchema;
  tags: string[];
  provenance?: SpecProvenance | null;
}

export interface WorkflowStepSpec {
  id: string;
  name: string;
  prompt: string;
  agent_id?: string | null;
  worker_preset?: string | null;
  continue_on_error: boolean;
}

export interface WorkflowSpec {
  schema_version: number;
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  steps: WorkflowStepSpec[];
  runtime_profile?: string | null;
  output_contract?: string | null;
  tags: string[];
  provenance?: SpecProvenance | null;
}

export interface SpecProvenance {
  source?: string | null;
  import_source?: string | null;
  notes?: string | null;
  imported_at?: number | null;
  draft_for?: string | null;
  review_status?: string | null;
  reviewed_at?: number | null;
}

export interface FactoryFieldChange {
  field: string;
  current?: string | null;
  proposed?: string | null;
}

export interface FactoryReview {
  kind: string;
  target_id: string;
  draft_id?: string | null;
  target_exists: boolean;
  review_status: string;
  suggested_action: string;
  summary: string;
  warnings: string[];
  changes: FactoryFieldChange[];
}

export interface AgentFactoryResult {
  spec: AgentSpec;
  review: FactoryReview;
}

export interface WorkflowFactoryResult {
  spec: WorkflowSpec;
  review: FactoryReview;
}

export interface SpecRunRecord {
  run_id: string;
  status: 'pending' | 'running' | 'completed' | 'failed';
  input: string;
  output?: string | null;
  error?: string | null;
  created_at: number;
  completed_at?: number | null;
}

export interface AgentRunRecord extends SpecRunRecord {
  agent_id: string;
  task_id?: string | null;
  workflow_run_id?: string | null;
}

export interface WorkflowRunRecord extends SpecRunRecord {
  workflow_id: string;
  steps_total: number;
  steps_completed: number;
  current_step_index?: number | null;
  current_step_id?: string | null;
  current_step_name?: string | null;
  retry_count: number;
  last_task_id?: string | null;
  resumable: boolean;
}

export interface WorkflowRunStepRecord {
  run_id: string;
  step_index: number;
  step_id: string;
  step_name: string;
  agent_id?: string | null;
  worker_preset?: string | null;
  status: 'pending' | 'running' | 'completed' | 'failed';
  prompt: string;
  task_id?: string | null;
  output?: string | null;
  error?: string | null;
  attempt_count: number;
  started_at: number;
  completed_at?: number | null;
}

export interface WorkflowRunDetail {
  run: WorkflowRunRecord;
  steps: WorkflowRunStepRecord[];
}

export interface SpecTemplateSummary {
  id: string;
  name: string;
  description: string;
}

export interface WorkerPreset {
  id: string;
  name: string;
  description: string;
  role: string;
  instructions: string;
  allowed_tools: string[];
  output_contract?: string | null;
  max_iterations?: number | null;
  max_steps: number;
  timeout_secs: number;
  memory_budget: number;
}

export type StarterCatalogKind =
  | 'agent_template'
  | 'workflow_template'
  | 'worker_preset'
  | 'channel_starter'
  | 'capability_pack';

export type StarterCatalogStatus = 'available' | 'needs_setup' | 'ready';

export interface StarterCatalogEntry {
  id: string;
  kind: StarterCatalogKind;
  name: string;
  description: string;
  official: boolean;
  status: StarterCatalogStatus;
  source: string;
  action_label: string;
  action_route?: string | null;
  command_hint?: string | null;
  tags: string[];
  components: string[];
  notes: string[];
}

export interface ChannelStatus {
  name: string;
  enabled: boolean;
  configured: boolean;
  healthy: boolean;
  summary: string;
}

export interface TelegramChannelStatus {
  name: string;
  enabled: boolean;
  configured: boolean;
  token_configured: boolean;
  can_receive: boolean;
  allowed_ids: number[];
  confirmation_chat_id?: number | null;
  api_base_url?: string | null;
  default_agent_id?: string | null;
  default_agent_name?: string | null;
  doctor: string[];
}

export interface TelegramChannelTestResponse {
  ok: boolean;
  message: string;
  bot_username?: string | null;
}

export type OnboardingStepState = 'complete' | 'action_required';

export interface OnboardingStep {
  id: string;
  title: string;
  state: OnboardingStepState;
  summary: string;
  action: string;
}

export interface OnboardingChecklist {
  completed_steps: number;
  total_steps: number;
  steps: OnboardingStep[];
}

export interface OverviewResponse {
  config: DaemonConfig;
  tasks: TaskSummary[];
  agent_runs: AgentRunRecord[];
  workflow_runs: WorkflowRunRecord[];
  approvals: ApprovalRequest[];
  services: ServiceStatus[];
  channels: ChannelStatus[];
  remote?: RemoteStatus | null;
  extensions: {
    installed: ExtensionRecord[];
    updates: ExtensionUpdateRecord[];
  };
  counts: {
    agents: number;
    workflows: number;
    extensions: number;
    pending_approvals: number;
  };
  queue: {
    pending: number;
    running: number;
  };
  local_load: NodeLoadSnapshot;
  remote_nodes: RemotePeer[];
  remote_candidates: RemoteDiscoveryCandidate[];
  zerotier?: ZeroTierStatus | null;
  health: RuntimeHealthSnapshot;
  onboarding: OnboardingChecklist;
  recent_logs: string[];
}

export interface LogStreamRecord {
  type: 'line' | 'error';
  line?: string;
  error?: string;
}

export interface BackupManifest {
  schema_version: number;
  created_at: number;
  rove_version: string;
  node_name: string;
  profile: string;
  secret_backend: string;
  config_path: string;
  data_dir: string;
  included_paths: string[];
  warnings: string[];
}

export interface BackupResponse {
  path: string;
  manifest: BackupManifest;
}

export interface MigrationArtifact {
  kind: string;
  path: string;
  supported: boolean;
  summary: string;
}

export interface MigrationReport {
  source: 'openclaw' | 'zeroclaw' | 'moltis';
  root: string;
  exists: boolean;
  config_files: string[];
  agent_candidates: MigrationArtifact[];
  workflow_candidates: MigrationArtifact[];
  detected_channels: string[];
  warnings: string[];
}

export interface MigrationImportResult {
  report: MigrationReport;
  imported_agents: string[];
  imported_workflows: string[];
  warnings: string[];
}

export interface PolicySummary {
  id: string;
  path: string;
  active: boolean;
  scope: string;
}

export interface PolicyExplainReport {
  task: string;
  domain: string;
  active_policies: string[];
  matched_hints: string[];
  system_prefix: string;
  system_suffix: string;
  verification_commands: string[];
  preferred_providers: string[];
  preferred_tools: string[];
  memory_tags: string[];
}

export interface ApprovalRequest {
  id: string;
  task_id: string;
  tool_name: string;
  risk_tier: number;
  summary: string;
  created_at: number;
  auto_resolve_after_secs?: number | null;
}

export interface ApprovalRule {
  id: string;
  action: 'allow' | 'require_approval';
  tool?: string | null;
  commands: string[];
  paths: string[];
  nodes: string[];
  channels: string[];
  risk_tier?: number | null;
  effect?: string | null;
}

export interface ApprovalRulesFile {
  rules: ApprovalRule[];
}

export interface ZeroTierStatus {
  enabled: boolean;
  installed: boolean;
  configured: boolean;
  token_configured: boolean;
  service_url: string;
  network_id?: string | null;
  managed_name_sync: boolean;
  service_online: boolean;
  joined: boolean;
  controller_access: boolean;
  node_id?: string | null;
  network_name?: string | null;
  network_status?: string | null;
  assigned_addresses: string[];
  transport_records: RemoteTransportRecord[];
  last_sync_at?: number | null;
  candidate_count: number;
  sync_state: string;
  message?: string | null;
}

export type BrowserProfileMode = 'managed_local' | 'attach_existing' | 'remote_cdp';
export type BrowserProfileReadiness = 'ready' | 'needs_setup' | 'warning';

export interface BrowserApprovalControls {
  require_approval_for_managed_launch: boolean;
  require_approval_for_existing_session_attach: boolean;
  require_approval_for_remote_cdp: boolean;
}

export interface BrowserProfileInput {
  id: string;
  name: string;
  enabled: boolean;
  mode: BrowserProfileMode;
  browser?: string | null;
  user_data_dir?: string | null;
  startup_url?: string | null;
  cdp_url?: string | null;
  notes?: string | null;
}

export interface BrowserProfileRecord extends BrowserProfileInput {
  is_default: boolean;
  readiness: BrowserProfileReadiness;
  approval_required: boolean;
  warnings: string[];
}

export interface BrowserSurfaceStatus {
  enabled: boolean;
  default_profile_id?: string | null;
  controls: BrowserApprovalControls;
  profiles: BrowserProfileRecord[];
  warnings: string[];
}

export interface BrowserSurfaceUpdate {
  enabled: boolean;
  default_profile_id?: string | null;
  controls: BrowserApprovalControls;
  profiles: BrowserProfileInput[];
}

export type VoiceEngineKind = 'native_os' | 'local_whisper' | 'local_piper';
export type VoiceEngineReadiness = 'ready' | 'needs_setup' | 'warning' | 'unsupported';
export type VoiceAssetStatus = 'none_required' | 'managed' | 'missing' | 'ready';
export type VoiceDeviceKind = 'input' | 'output';
export type MemoryMode = 'graph_only' | 'always_on';
export type MemoryGraphEnrichment = 'deterministic' | 'deterministic_plus_llm';
export type MemoryBundleStrategy = 'adaptive';
export type MemoryRetrievalAssist = 'off' | 'rerank' | 'compress';
export type MemoryAdapterMode = 'off' | 'auto' | 'required';

export interface VoicePolicyControls {
  require_approval_for_tts: boolean;
  require_approval_for_stt: boolean;
  allow_remote_audio_input: boolean;
  allow_remote_audio_output: boolean;
  persist_transcripts: boolean;
}

export interface VoiceEngineInput {
  kind: VoiceEngineKind;
  enabled: boolean;
  model?: string | null;
  voice?: string | null;
  runtime_path?: string | null;
  asset_dir?: string | null;
  notes?: string | null;
}

export interface VoiceDeviceRecord {
  id: string;
  name: string;
  kind: VoiceDeviceKind;
  default: boolean;
  available: boolean;
}

export interface VoiceRuntimeStatus {
  system_id: string;
  installed: boolean;
  enabled: boolean;
  version?: string | null;
  artifact_path?: string | null;
  warnings: string[];
}

export interface VoiceEngineRecord extends VoiceEngineInput {
  id: string;
  name: string;
  installed: boolean;
  supports_input: boolean;
  supports_output: boolean;
  active_input: boolean;
  active_output: boolean;
  asset_status: VoiceAssetStatus;
  readiness: VoiceEngineReadiness;
  approval_required_for_input: boolean;
  approval_required_for_output: boolean;
  warnings: string[];
}

export interface VoiceSurfaceStatus {
  enabled: boolean;
  runtime: VoiceRuntimeStatus;
  active_input_engine?: VoiceEngineKind | null;
  active_output_engine?: VoiceEngineKind | null;
  selected_input_device_id?: string | null;
  selected_output_device_id?: string | null;
  policy: VoicePolicyControls;
  devices: VoiceDeviceRecord[];
  engines: VoiceEngineRecord[];
  warnings: string[];
}

export interface VoiceSurfaceUpdate {
  enabled: boolean;
  active_input_engine?: VoiceEngineKind | null;
  active_output_engine?: VoiceEngineKind | null;
  selected_input_device_id?: string | null;
  selected_output_device_id?: string | null;
  policy: VoicePolicyControls;
  engines: VoiceEngineInput[];
}

export interface VoiceEngineInstallRequest {
  engine: VoiceEngineKind;
  model?: string | null;
  voice?: string | null;
  runtime_path?: string | null;
  notes?: string | null;
}

export interface VoiceEngineSelectionRequest {
  engine: VoiceEngineKind;
}

export interface VoiceOutputTestRequest {
  text: string;
  voice?: string | null;
}

export interface VoiceTestResult {
  ok: boolean;
  engine: VoiceEngineKind;
  message: string;
}

export interface MemoryGraphRepoStatus {
  repo_name: string;
  repo_path: string;
  db_path: string;
  available: boolean;
  imported: boolean;
  stale: boolean;
  nodes: number;
  edges: number;
  files: number;
  last_updated?: string | null;
  built_branch?: string | null;
  built_commit?: string | null;
  current_branch?: string | null;
  current_commit?: string | null;
  message?: string | null;
}

export interface MemoryGraphWorkspaceStatus {
  healthy: boolean;
  available_count: number;
  imported_count: number;
  stale_count: number;
  repos: MemoryGraphRepoStatus[];
}

export interface MemorySurfaceStatus {
  mode: MemoryMode;
  bundle_strategy: MemoryBundleStrategy;
  retrieval_assist: MemoryRetrievalAssist;
  graph_enrichment: MemoryGraphEnrichment;
  scope: string;
  code_graph_required: boolean;
  code_adapter_mode: MemoryAdapterMode;
  always_on_enabled: boolean;
  persist_pinned_facts: boolean;
  persist_task_traces: boolean;
  graph_status: MemoryGraphWorkspaceStatus;
  graph_stats: Record<string, number>;
  memory_stats: MemorySurfaceStats;
  warnings: string[];
}

export interface MemorySurfaceUpdate {
  mode?: MemoryMode | null;
  bundle_strategy?: MemoryBundleStrategy | null;
  retrieval_assist?: MemoryRetrievalAssist | null;
  graph_enrichment?: MemoryGraphEnrichment | null;
  code_graph_required?: boolean | null;
  code_adapter_mode?: MemoryAdapterMode | null;
  persist_pinned_facts?: boolean | null;
  persist_task_traces?: boolean | null;
}

export interface GraphPathHit {
  summary: string;
  path: string[];
  source_kinds: string[];
  source_refs: string[];
  confidence: number;
  score: number;
}

export interface MemoryHit {
  id: string;
  source: string;
  content: string;
  rank: number;
  hit_type: 'episodic' | 'insight' | 'knowledge_graph' | 'task_trace' | 'fact';
  importance: number;
  created_at: number;
  final_score: number;
}

export interface MemoryExplainResponse {
  intent: string;
  mode: MemoryMode;
  sources: string[];
  fallback_reason?: string | null;
  graph_paths_used: number;
  memory_graph_hits_used: number;
  task_trace_hits_used: number;
  llm_enrichment_enabled: boolean;
}

export interface MemorySurfaceStats {
  facts: number;
  task_traces: number;
  episodic: number;
  insights: number;
  total_episodic: number;
  embedded_episodic: number;
  embedding_coverage_pct: number;
  memory_graph_edges: number;
  edge_types: Record<string, number>;
}

export interface MemoryQueryRequest {
  question: string;
  explain: boolean;
  domain?: string | null;
}

export interface MemoryGraphHit {
  id: string;
  content: string;
  memory_kind: string;
  importance: number;
  domain: string;
  created_at: number;
  /** IDs of nodes traversed from seed to this node, inclusive */
  path: string[];
  /** Edge types along the path */
  path_edge_types: string[];
  depth: number;
  /** importance × decay^depth × edge weight */
  graph_score: number;
}

export interface MemoryQueryResponse {
  facts: MemoryHit[];
  preferences: MemoryHit[];
  warnings: MemoryHit[];
  errors: MemoryHit[];
  graph_paths: GraphPathHit[];
  memory_graph_hits: MemoryGraphHit[];
  episodic_hits: MemoryHit[];
  insight_hits: MemoryHit[];
  task_trace_hits: MemoryHit[];
  project_context?: string | null;
  explain?: MemoryExplainResponse | null;
}

export interface MemoryGraphInspectResponse {
  entity?: string | null;
  graph_status: MemoryGraphWorkspaceStatus;
  graph_stats: Record<string, number>;
  paths: GraphPathHit[];
}

export interface MemoryIngestRequest {
  note: string;
  domain?: string | null;
}

export interface MemoryBackfillRequest {
  batch_size?: number | null;
}

export interface MemoryBackfillResponse {
  backfilled: number;
  status: MemorySurfaceStatus;
}

export interface DispatchBrainView {
  root: string;
  active?: string | null;
  installed: string[];
  source?: string | null;
}

export interface CreateTaskResponse {
  task_id: string;
  status: string;
}

export interface ExecuteTaskResponse {
  success?: boolean;
  task_id?: string | null;
  status: string;
  answer?: string | null;
  provider?: string | null;
  duration_ms?: number | null;
  message?: string | null;
}

export type DaemonEvent =
  | { type: 'auth.locked' }
  | { type: 'daemon.status'; state: string }
  | { type: 'task.created'; task_id: string }
  | { type: 'task.event'; task_id: string; event: unknown }
  | { type: 'task.completed'; task_id: string; result?: string }
  | { type: 'approval.required'; task_id: string; risk: string }
  | { type: 'remote.node.updated'; node_name: string };

export const DEFAULT_DAEMON_PORT = 43177;
const LEGACY_DAEMON_PORTS = [3727, 47630];

const TOKEN_KEY = 'rove_webui_access_token';
const PORT_KEY = 'rove_webui_daemon_port';

function buildLoopbackBaseUrls(port: number): string[] {
  return [
    `http://localhost:${port}`,
    `https://localhost:${port}`,
    `http://127.0.0.1:${port}`,
    `https://127.0.0.1:${port}`,
  ];
}

function normalizePort(value: number | null | undefined): number | null {
  if (value == null) {
    return null;
  }
  if (!Number.isInteger(value) || value < 1 || value > 65535) {
    throw new Error('Daemon port must be an integer between 1 and 65535');
  }
  return value;
}

export function readStoredToken(): string | null {
  if (typeof window === 'undefined') {
    return null;
  }
  return window.sessionStorage.getItem(TOKEN_KEY);
}

export function readStoredDaemonPort(): number | null {
  if (typeof window === 'undefined') {
    return null;
  }

  const value = window.localStorage.getItem(PORT_KEY);
  if (!value) {
    return null;
  }

  const parsed = Number(value);
  return Number.isInteger(parsed) && parsed >= 1 && parsed <= 65535 ? parsed : null;
}

export function writeStoredToken(token: string | null) {
  if (typeof window === 'undefined') {
    return;
  }
  if (token) {
    window.sessionStorage.setItem(TOKEN_KEY, token);
  } else {
    window.sessionStorage.removeItem(TOKEN_KEY);
  }
}

export function writeStoredDaemonPort(port: number | null) {
  if (typeof window === 'undefined') {
    return;
  }

  const normalized = normalizePort(port);
  if (normalized == null) {
    window.localStorage.removeItem(PORT_KEY);
  } else {
    window.localStorage.setItem(PORT_KEY, String(normalized));
  }
}

function configuredBaseUrls(portOverride?: number | null): string[] {
  const configured = process.env.NEXT_PUBLIC_ROVE_DAEMON_URLS
    ?.split(',')
    .map((value) => value.trim())
    .filter(Boolean);
  const defaults = [DEFAULT_DAEMON_PORT, ...LEGACY_DAEMON_PORTS].flatMap((port) =>
    buildLoopbackBaseUrls(port),
  );

  return Array.from(
    new Set([
      ...(portOverride ? buildLoopbackBaseUrls(portOverride) : []),
      ...(configured ?? []),
      ...defaults,
    ]),
  );
}

export class DaemonError extends Error {
  readonly status?: number;
  readonly code?: string;

  constructor(message: string, status?: number, code?: string) {
    super(message);
    this.name = 'DaemonError';
    this.status = status;
    this.code = code;
  }
}

export class RoveDaemonClient {
  private token?: string;
  private preferredBaseUrl?: string;
  private baseUrls: string[];

  constructor(token?: string) {
    this.token = token;
    this.baseUrls = configuredBaseUrls(readStoredDaemonPort());
  }

  setToken(token?: string) {
    this.token = token;
  }

  setPortOverride(port?: number | null) {
    const normalized = normalizePort(port ?? null);
    writeStoredDaemonPort(normalized);
    this.preferredBaseUrl = undefined;
    this.baseUrls = configuredBaseUrls(normalized);
  }

  currentBaseUrl(): string | null {
    return this.preferredBaseUrl ?? this.baseUrls[0] ?? null;
  }

  currentPort(): number | null {
    const baseUrl = this.currentBaseUrl();
    if (!baseUrl) {
      return null;
    }

    try {
      const parsed = new URL(baseUrl);
      const port = Number(parsed.port);
      return Number.isInteger(port) && port > 0 ? port : null;
    } catch {
      return null;
    }
  }

  async hello(): Promise<DaemonHello> {
    return this.request<DaemonHello>('/v1/hello');
  }

  async authSetup(password: string, nodeName: string, mode: string): Promise<SessionInfo> {
    return this.request<SessionInfo>('/v1/auth/setup', {
      method: 'POST',
      body: JSON.stringify({
        password,
        node_name: nodeName,
        mode,
      }),
    });
  }

  async authLogin(password: string): Promise<SessionInfo> {
    return this.request<SessionInfo>('/v1/auth/login', {
      method: 'POST',
      body: JSON.stringify({ password }),
    });
  }

  async authStatus(): Promise<AuthStatus> {
    return this.request<AuthStatus>('/v1/auth/status');
  }

  async authLock(): Promise<void> {
    await this.request<void>('/v1/auth/lock', { method: 'POST' });
  }

  async authReauth(password: string): Promise<AuthStatus> {
    return this.request<AuthStatus>('/v1/auth/reauth', {
      method: 'POST',
      body: JSON.stringify({ password }),
    });
  }

  async listTasks(): Promise<TaskSummary[]> {
    return this.request<TaskSummary[]>('/v1/tasks');
  }

  async getConfig(): Promise<DaemonConfig> {
    return this.request<DaemonConfig>('/v1/config');
  }

  async updateConfig(payload: Partial<DaemonConfig>): Promise<DaemonConfig> {
    return this.request<DaemonConfig>('/v1/config', {
      method: 'POST',
      body: JSON.stringify(payload),
    });
  }

  async getBrowserSurface(): Promise<BrowserSurfaceStatus> {
    return this.request<BrowserSurfaceStatus>('/v1/browser');
  }

  async updateBrowserSurface(payload: BrowserSurfaceUpdate): Promise<BrowserSurfaceStatus> {
    return this.request<BrowserSurfaceStatus>('/v1/browser', {
      method: 'PUT',
      body: JSON.stringify(payload),
    });
  }

  async getVoiceSurface(): Promise<VoiceSurfaceStatus> {
    return this.request<VoiceSurfaceStatus>('/v1/voice');
  }

  async getMemorySurface(): Promise<MemorySurfaceStatus> {
    return this.request<MemorySurfaceStatus>('/v1/memory');
  }

  async updateMemorySurface(payload: MemorySurfaceUpdate): Promise<MemorySurfaceStatus> {
    return this.request<MemorySurfaceStatus>('/v1/memory', {
      method: 'PUT',
      body: JSON.stringify(payload),
    });
  }

  async queryMemory(payload: MemoryQueryRequest): Promise<MemoryQueryResponse> {
    return this.request<MemoryQueryResponse>('/v1/memory/query', {
      method: 'POST',
      body: JSON.stringify(payload),
    });
  }

  async inspectMemoryGraph(entity?: string | null): Promise<MemoryGraphInspectResponse> {
    const suffix = entity ? `?entity=${encodeURIComponent(entity)}` : '';
    return this.request<MemoryGraphInspectResponse>(`/v1/memory/graph${suffix}`);
  }

  async reindexMemory(): Promise<MemorySurfaceStatus> {
    return this.request<MemorySurfaceStatus>('/v1/memory/reindex', {
      method: 'POST',
      body: JSON.stringify({}),
    });
  }

  async backfillMemory(payload: MemoryBackfillRequest): Promise<MemoryBackfillResponse> {
    return this.request<MemoryBackfillResponse>('/v1/memory/backfill', {
      method: 'POST',
      body: JSON.stringify(payload),
    });
  }

  async listMemoryAdapters(): Promise<MemoryGraphWorkspaceStatus> {
    return this.request<MemoryGraphWorkspaceStatus>('/v1/memory/adapters');
  }

  async refreshMemoryAdapters(): Promise<MemoryGraphWorkspaceStatus> {
    return this.request<MemoryGraphWorkspaceStatus>('/v1/memory/adapters/refresh', {
      method: 'POST',
      body: JSON.stringify({}),
    });
  }

  async ingestMemoryNote(payload: MemoryIngestRequest): Promise<MemoryHit> {
    return this.request<MemoryHit>('/v1/memory/ingest', {
      method: 'POST',
      body: JSON.stringify(payload),
    });
  }

  async updateVoiceSurface(payload: VoiceSurfaceUpdate): Promise<VoiceSurfaceStatus> {
    return this.request<VoiceSurfaceStatus>('/v1/voice', {
      method: 'PUT',
      body: JSON.stringify(payload),
    });
  }

  async installVoiceEngine(payload: VoiceEngineInstallRequest): Promise<VoiceSurfaceStatus> {
    return this.request<VoiceSurfaceStatus>('/v1/voice/install', {
      method: 'POST',
      body: JSON.stringify(payload),
    });
  }

  async uninstallVoiceEngine(payload: VoiceEngineSelectionRequest): Promise<VoiceSurfaceStatus> {
    return this.request<VoiceSurfaceStatus>('/v1/voice/uninstall', {
      method: 'POST',
      body: JSON.stringify(payload),
    });
  }

  async activateVoiceInput(payload: VoiceEngineSelectionRequest): Promise<VoiceSurfaceStatus> {
    return this.request<VoiceSurfaceStatus>('/v1/voice/activate-input', {
      method: 'POST',
      body: JSON.stringify(payload),
    });
  }

  async activateVoiceOutput(payload: VoiceEngineSelectionRequest): Promise<VoiceSurfaceStatus> {
    return this.request<VoiceSurfaceStatus>('/v1/voice/activate-output', {
      method: 'POST',
      body: JSON.stringify(payload),
    });
  }

  async testVoiceInput(): Promise<VoiceTestResult> {
    return this.request<VoiceTestResult>('/v1/voice/test-input', {
      method: 'POST',
      body: JSON.stringify({}),
    });
  }

  async testVoiceOutput(payload: VoiceOutputTestRequest): Promise<VoiceTestResult> {
    return this.request<VoiceTestResult>('/v1/voice/test-output', {
      method: 'POST',
      body: JSON.stringify(payload),
    });
  }

  async listServices(): Promise<ServiceStatus[]> {
    return this.request<ServiceStatus[]>('/v1/services');
  }

  async getOverview(): Promise<OverviewResponse> {
    return this.request<OverviewResponse>('/v1/overview');
  }

  async getHealthSnapshot(): Promise<RuntimeHealthSnapshot> {
    return this.request<RuntimeHealthSnapshot>('/v1/health/snapshot');
  }

  async getRecentLogs(): Promise<{ lines: string[] }> {
    return this.request<{ lines: string[] }>('/v1/logs/recent');
  }

  streamLogs(handlers: {
    onOpen?: () => void;
    onLine?: (line: string) => void;
    onError?: (message: string) => void;
    onClose?: () => void;
  }): () => void {
    const baseUrl = this.currentBaseUrl();
    if (!baseUrl || !this.token) {
      throw new DaemonError('Missing daemon session');
    }

    const controller = new AbortController();

    void (async () => {
      let reader: ReadableStreamDefaultReader<Uint8Array> | null = null;
      try {
        const response = await fetch(`${baseUrl}/v1/logs/stream`, {
          method: 'GET',
          headers: this.headers(undefined, false),
          cache: 'no-store',
          signal: controller.signal,
        });

        if (!response.ok) {
          let message = response.statusText;
          try {
            const body = (await response.json()) as { error?: string };
            message = body.error ?? message;
          } catch {
            // Ignore non-JSON stream errors.
          }
          throw new DaemonError(message, response.status);
        }

        handlers.onOpen?.();
        reader = response.body?.getReader() ?? null;
        if (!reader) {
          throw new DaemonError('Daemon log stream did not provide a readable body');
        }

        const decoder = new TextDecoder();
        let buffer = '';

        while (true) {
          const { value, done } = await reader.read();
          if (done) {
            break;
          }
          buffer += decoder.decode(value, { stream: true });

          let newlineIndex = buffer.indexOf('\n');
          while (newlineIndex !== -1) {
            const raw = buffer.slice(0, newlineIndex).trim();
            buffer = buffer.slice(newlineIndex + 1);
            if (raw) {
              const record = JSON.parse(raw) as LogStreamRecord;
              if (record.type === 'line' && record.line) {
                handlers.onLine?.(record.line);
              } else if (record.type === 'error') {
                handlers.onError?.(record.error ?? 'Daemon log stream failed');
              }
            }
            newlineIndex = buffer.indexOf('\n');
          }
        }

        handlers.onClose?.();
      } catch (error) {
        if (controller.signal.aborted) {
          handlers.onClose?.();
          return;
        }
        handlers.onError?.(error instanceof Error ? error.message : 'Daemon log stream failed');
      } finally {
        void reader?.cancel().catch(() => undefined);
      }
    })();

    return () => controller.abort();
  }

  async exportBackup(path?: string, force = false): Promise<BackupResponse> {
    return this.request<BackupResponse>('/v1/backups/export', {
      method: 'POST',
      body: JSON.stringify({ path, force }),
    });
  }

  async restoreBackup(path: string, force = false): Promise<BackupResponse> {
    return this.request<BackupResponse>('/v1/backups/restore', {
      method: 'POST',
      body: JSON.stringify({ path, force }),
    });
  }

  async inspectMigration(
    source: 'openclaw' | 'zeroclaw' | 'moltis',
    path?: string,
  ): Promise<MigrationReport> {
    return this.request<MigrationReport>(`/v1/migrate/${encodeURIComponent(source)}/inspect`, {
      method: 'POST',
      body: JSON.stringify({ path }),
    });
  }

  async importMigration(
    source: 'openclaw' | 'zeroclaw' | 'moltis',
    path?: string,
  ): Promise<MigrationImportResult> {
    return this.request<MigrationImportResult>(`/v1/migrate/${encodeURIComponent(source)}/import`, {
      method: 'POST',
      body: JSON.stringify({ path }),
    });
  }

  async serviceInstallStatus(): Promise<ServiceInstallStatus> {
    return this.request<ServiceInstallStatus>('/v1/services/install/status');
  }

  async installService(
    mode: 'login' | 'boot',
    profile?: 'desktop' | 'headless',
    port?: number,
  ): Promise<ServiceInstallState> {
    return this.request<ServiceInstallState>('/v1/services/install', {
      method: 'POST',
      body: JSON.stringify({ mode, profile, port }),
    });
  }

  async uninstallService(mode: 'login' | 'boot'): Promise<void> {
    await this.request<void>(`/v1/services/install/${encodeURIComponent(mode)}`, {
      method: 'DELETE',
    });
  }

  async listBrains(): Promise<{ dispatch: DispatchBrainView }> {
    return this.request<{ dispatch: DispatchBrainView }>('/v1/brains');
  }

  async useDispatchBrain(model: string): Promise<DispatchBrainView> {
    return this.request<DispatchBrainView>('/v1/brains/dispatch/use', {
      method: 'POST',
      body: JSON.stringify({ model }),
    });
  }

  async setServiceEnabled(name: string, enabled: boolean): Promise<ServiceStatus> {
    return this.request<ServiceStatus>(`/v1/services/${encodeURIComponent(name)}/${enabled ? 'enable' : 'disable'}`, {
      method: 'POST',
    });
  }

  async listChannels(): Promise<ChannelStatus[]> {
    return this.request<ChannelStatus[]>('/v1/channels');
  }

  async listStarters(): Promise<StarterCatalogEntry[]> {
    return this.request<StarterCatalogEntry[]>('/v1/starters');
  }

  async getTelegramChannel(): Promise<TelegramChannelStatus> {
    return this.request<TelegramChannelStatus>('/v1/channels/telegram');
  }

  async setupTelegramChannel(input: {
    token?: string;
    allowed_ids?: number[];
    confirmation_chat_id?: number | null;
    api_base_url?: string | null;
    default_agent_id?: string | null;
  }): Promise<TelegramChannelStatus> {
    return this.request<TelegramChannelStatus>('/v1/channels/telegram/setup', {
      method: 'POST',
      body: JSON.stringify(input),
    });
  }

  async enableTelegramChannel(): Promise<TelegramChannelStatus> {
    return this.request<TelegramChannelStatus>('/v1/channels/telegram/enable', {
      method: 'POST',
    });
  }

  async disableTelegramChannel(): Promise<TelegramChannelStatus> {
    return this.request<TelegramChannelStatus>('/v1/channels/telegram/disable', {
      method: 'POST',
    });
  }

  async testTelegramChannel(): Promise<TelegramChannelTestResponse> {
    return this.request<TelegramChannelTestResponse>('/v1/channels/telegram/test', {
      method: 'POST',
    });
  }

  async listExtensions(): Promise<ExtensionRecord[]> {
    return this.request<ExtensionRecord[]>('/v1/extensions');
  }

  async listExtensionCatalog(): Promise<CatalogExtensionRecord[]> {
    return this.request<CatalogExtensionRecord[]>('/v1/extensions/catalog');
  }

  async getExtensionCatalog(id: string): Promise<CatalogExtensionRecord> {
    return this.request<CatalogExtensionRecord>(`/v1/extensions/catalog/${encodeURIComponent(id)}`);
  }

  async refreshExtensionCatalog(): Promise<CatalogExtensionRecord[]> {
    return this.request<CatalogExtensionRecord[]>('/v1/extensions/catalog/refresh', {
      method: 'POST',
    });
  }

  async listExtensionUpdates(): Promise<ExtensionUpdateRecord[]> {
    return this.request<ExtensionUpdateRecord[]>('/v1/extensions/updates');
  }

  async installExtension(input: {
    kind?: string;
    source: string;
    registry?: string;
    version?: string;
  }): Promise<ExtensionRecord> {
    return this.request<ExtensionRecord>('/v1/extensions/install', {
      method: 'POST',
      body: JSON.stringify(input),
    });
  }

  async upgradeExtension(input: {
    kind?: string;
    source: string;
    registry?: string;
    version?: string;
  }): Promise<ExtensionRecord> {
    return this.request<ExtensionRecord>('/v1/extensions/upgrade', {
      method: 'POST',
      body: JSON.stringify(input),
    });
  }

  async setExtensionEnabled(kind: string, name: string, enabled: boolean): Promise<ExtensionRecord> {
    return this.request<ExtensionRecord>(
      `/v1/extensions/${encodeURIComponent(kind)}/${encodeURIComponent(name)}/${enabled ? 'enable' : 'disable'}`,
      { method: 'POST' },
    );
  }

  async removeExtension(kind: string, name: string): Promise<void> {
    await this.request<void>(`/v1/extensions/${encodeURIComponent(kind)}/${encodeURIComponent(name)}`, {
      method: 'DELETE',
    });
  }

  async listPolicies(): Promise<PolicySummary[]> {
    return this.request<PolicySummary[]>('/v1/policies');
  }

  async explainPolicy(task: string): Promise<PolicyExplainReport> {
    return this.request<PolicyExplainReport>('/v1/policies/explain', {
      method: 'POST',
      body: JSON.stringify({ task }),
    });
  }

  async setPolicyEnabled(name: string, enabled: boolean): Promise<void> {
    await this.request<void>(`/v1/policies/${encodeURIComponent(name)}/${enabled ? 'enable' : 'disable'}`, {
      method: 'POST',
    });
  }

  async addPolicy(name: string, scope: 'user' | 'workspace' | 'project'): Promise<{ path: string }> {
    return this.request<{ path: string }>('/v1/policies', {
      method: 'POST',
      body: JSON.stringify({ name, scope }),
    });
  }

  async removePolicy(name: string): Promise<void> {
    await this.request<void>(`/v1/policies/${encodeURIComponent(name)}`, {
      method: 'DELETE',
    });
  }

  async listRemoteNodes(): Promise<RemotePeer[]> {
    return this.request<RemotePeer[]>('/v1/remote/nodes');
  }

  async remoteStatus(): Promise<RemoteStatus> {
    return this.request<RemoteStatus>('/v1/remote/status');
  }

  async trustRemoteNode(name: string): Promise<void> {
    await this.request<void>(`/v1/remote/nodes/${encodeURIComponent(name)}/trust`, {
      method: 'POST',
    });
  }

  async unpairRemoteNode(name: string): Promise<void> {
    await this.request<void>(`/v1/remote/nodes/${encodeURIComponent(name)}`, {
      method: 'DELETE',
    });
  }

  async listApprovals(): Promise<ApprovalRequest[]> {
    return this.request<ApprovalRequest[]>('/v1/approvals');
  }

  async listApprovalRules(): Promise<ApprovalRulesFile> {
    return this.request<ApprovalRulesFile>('/v1/approvals/rules');
  }

  async addApprovalRule(rule: ApprovalRule): Promise<ApprovalRulesFile> {
    return this.request<ApprovalRulesFile>('/v1/approvals/rules', {
      method: 'POST',
      body: JSON.stringify(rule),
    });
  }

  async removeApprovalRule(id: string): Promise<void> {
    await this.request<void>(`/v1/approvals/rules/${encodeURIComponent(id)}`, {
      method: 'DELETE',
    });
  }

  async resolveApproval(id: string, approved: boolean): Promise<void> {
    await this.request<void>(`/v1/approvals/${encodeURIComponent(id)}/resolve`, {
      method: 'POST',
      body: JSON.stringify({ approved }),
    });
  }

  async zeroTierStatus(): Promise<ZeroTierStatus> {
    return this.request<ZeroTierStatus>('/v1/remote/transports/zerotier');
  }

  async zeroTierInstall(): Promise<ZeroTierStatus> {
    return this.request<ZeroTierStatus>('/v1/remote/transports/zerotier/install', {
      method: 'POST',
    });
  }

  async zeroTierUninstall(): Promise<ZeroTierStatus> {
    return this.request<ZeroTierStatus>('/v1/remote/transports/zerotier/uninstall', {
      method: 'POST',
    });
  }

  async zeroTierSetup(input: {
    network_id: string;
    api_token_key?: string;
    managed_name_sync?: boolean;
  }): Promise<ZeroTierStatus> {
    return this.request<ZeroTierStatus>('/v1/remote/transports/zerotier/setup', {
      method: 'POST',
      body: JSON.stringify(input),
    });
  }

  async zeroTierJoin(networkId?: string): Promise<ZeroTierStatus> {
    return this.request<ZeroTierStatus>('/v1/remote/transports/zerotier', {
      method: 'POST',
      body: JSON.stringify({ network_id: networkId }),
    });
  }

  async zeroTierRefresh(): Promise<ZeroTierStatus> {
    return this.request<ZeroTierStatus>('/v1/remote/transports/zerotier/refresh', {
      method: 'POST',
    });
  }

  async listRemoteDiscovery(): Promise<RemoteDiscoveryCandidate[]> {
    return this.request<RemoteDiscoveryCandidate[]>('/v1/remote/discover');
  }

  async trustRemoteCandidate(candidateId: string): Promise<RemoteDiscoveryCandidate> {
    return this.request<RemoteDiscoveryCandidate>(`/v1/remote/discover/${encodeURIComponent(candidateId)}/trust`, {
      method: 'POST',
    });
  }

  async createTask(
    input: string,
    options?: { parallel?: boolean; isolate?: 'none' | 'worktree' | 'snapshot'; node?: string },
  ): Promise<CreateTaskResponse> {
    return this.request<CreateTaskResponse>('/v1/tasks', {
      method: 'POST',
      body: JSON.stringify({
        input,
        parallel: options?.parallel ?? false,
        isolate: options?.isolate,
        node: options?.node,
      }),
    });
  }

  async listAgents(): Promise<AgentSpec[]> {
    return this.request<AgentSpec[]>('/v1/agents');
  }

  async listAgentTemplates(): Promise<SpecTemplateSummary[]> {
    return this.request<SpecTemplateSummary[]>('/v1/agents/templates');
  }

  async previewAgentFactory(input: {
    requirement: string;
    template_id?: string;
    id?: string;
    name?: string;
  }): Promise<AgentFactoryResult> {
    return this.request<AgentFactoryResult>('/v1/agents/factory/preview', {
      method: 'POST',
      body: JSON.stringify(input),
    });
  }

  async createAgentFactory(input: {
    requirement: string;
    template_id?: string;
    id?: string;
    name?: string;
  }): Promise<AgentFactoryResult> {
    return this.request<AgentFactoryResult>('/v1/agents/factory/create', {
      method: 'POST',
      body: JSON.stringify(input),
    });
  }

  async createAgentFromTask(taskId: string, input?: {
    id?: string;
    name?: string;
  }): Promise<AgentFactoryResult> {
    return this.request<AgentFactoryResult>(`/v1/agents/from-task/${encodeURIComponent(taskId)}`, {
      method: 'POST',
      body: JSON.stringify(input ?? {}),
    });
  }

  async getAgentReview(id: string): Promise<FactoryReview> {
    return this.request<FactoryReview>(`/v1/agents/${encodeURIComponent(id)}/review`);
  }

  async approveAgentDraft(id: string): Promise<AgentSpec> {
    return this.request<AgentSpec>(`/v1/agents/${encodeURIComponent(id)}/approve`, {
      method: 'POST',
    });
  }

  async saveAgent(spec: AgentSpec): Promise<AgentSpec> {
    const path = spec.id ? `/v1/agents/${encodeURIComponent(spec.id)}` : '/v1/agents';
    return this.request<AgentSpec>(path, {
      method: spec.id ? 'PUT' : 'POST',
      body: JSON.stringify(spec),
    });
  }

  async removeAgent(id: string): Promise<void> {
    await this.request<void>(`/v1/agents/${encodeURIComponent(id)}`, {
      method: 'DELETE',
    });
  }

  async runAgent(id: string, input: string): Promise<ExecuteTaskResponse> {
    return this.request<ExecuteTaskResponse>(`/v1/agents/${encodeURIComponent(id)}/run`, {
      method: 'POST',
      body: JSON.stringify({ input }),
    });
  }

  async listAgentRuns(): Promise<AgentRunRecord[]> {
    return this.request<AgentRunRecord[]>('/v1/agents/runs');
  }

  async listWorkflows(): Promise<WorkflowSpec[]> {
    return this.request<WorkflowSpec[]>('/v1/workflows');
  }

  async listWorkflowTemplates(): Promise<SpecTemplateSummary[]> {
    return this.request<SpecTemplateSummary[]>('/v1/workflows/templates');
  }

  async listWorkerPresets(): Promise<WorkerPreset[]> {
    return this.request<WorkerPreset[]>('/v1/workers/presets');
  }

  async previewWorkflowFactory(input: {
    requirement: string;
    template_id?: string;
    id?: string;
    name?: string;
  }): Promise<WorkflowFactoryResult> {
    return this.request<WorkflowFactoryResult>('/v1/workflows/factory/preview', {
      method: 'POST',
      body: JSON.stringify(input),
    });
  }

  async createWorkflowFactory(input: {
    requirement: string;
    template_id?: string;
    id?: string;
    name?: string;
  }): Promise<WorkflowFactoryResult> {
    return this.request<WorkflowFactoryResult>('/v1/workflows/factory/create', {
      method: 'POST',
      body: JSON.stringify(input),
    });
  }

  async createWorkflowFromTask(taskId: string, input?: {
    id?: string;
    name?: string;
  }): Promise<WorkflowFactoryResult> {
    return this.request<WorkflowFactoryResult>(`/v1/workflows/from-task/${encodeURIComponent(taskId)}`, {
      method: 'POST',
      body: JSON.stringify(input ?? {}),
    });
  }

  async getWorkflowReview(id: string): Promise<FactoryReview> {
    return this.request<FactoryReview>(`/v1/workflows/${encodeURIComponent(id)}/review`);
  }

  async approveWorkflowDraft(id: string): Promise<WorkflowSpec> {
    return this.request<WorkflowSpec>(`/v1/workflows/${encodeURIComponent(id)}/approve`, {
      method: 'POST',
    });
  }

  async saveWorkflow(spec: WorkflowSpec): Promise<WorkflowSpec> {
    const path = spec.id ? `/v1/workflows/${encodeURIComponent(spec.id)}` : '/v1/workflows';
    return this.request<WorkflowSpec>(path, {
      method: spec.id ? 'PUT' : 'POST',
      body: JSON.stringify(spec),
    });
  }

  async removeWorkflow(id: string): Promise<void> {
    await this.request<void>(`/v1/workflows/${encodeURIComponent(id)}`, {
      method: 'DELETE',
    });
  }

  async runWorkflow(id: string, input: string): Promise<ExecuteTaskResponse> {
    return this.request<ExecuteTaskResponse>(`/v1/workflows/${encodeURIComponent(id)}/run`, {
      method: 'POST',
      body: JSON.stringify({ input }),
    });
  }

  async listWorkflowRuns(): Promise<WorkflowRunRecord[]> {
    return this.request<WorkflowRunRecord[]>('/v1/workflows/runs');
  }

  async getWorkflowRun(runId: string): Promise<WorkflowRunDetail> {
    return this.request<WorkflowRunDetail>(`/v1/workflows/runs/${encodeURIComponent(runId)}`);
  }

  async resumeWorkflowRun(runId: string): Promise<ExecuteTaskResponse> {
    return this.request<ExecuteTaskResponse>(
      `/v1/workflows/runs/${encodeURIComponent(runId)}/resume`,
      {
        method: 'POST',
      },
    );
  }

  connectEvents(onEvent: (event: DaemonEvent) => void): WebSocket {
    const baseUrl = this.currentBaseUrl();
    if (!baseUrl || !this.token) {
      throw new DaemonError('Missing daemon session');
    }

    const wsBase = baseUrl.startsWith('https://')
      ? `wss://${baseUrl.slice('https://'.length)}`
      : `ws://${baseUrl.slice('http://'.length)}`;
    const ws = new WebSocket(
      `${wsBase}/v1/events/ws?token=${encodeURIComponent(this.token)}`,
    );

    ws.onopen = () => {
      ws.send(JSON.stringify({ type: 'subscribe', topic: 'tasks' }));
      ws.send(JSON.stringify({ type: 'subscribe', topic: 'daemon' }));
    };
    ws.onmessage = (message) => {
      try {
        onEvent(JSON.parse(message.data) as DaemonEvent);
      } catch (error) {
        console.error('Failed to parse daemon event', error);
      }
    };

    return ws;
  }

  private async request<T>(path: string, init: RequestInit = {}): Promise<T> {
    const errors: string[] = [];
    const orderedBaseUrls = this.orderedBaseUrls();

    for (const baseUrl of orderedBaseUrls) {
      try {
        const response = await fetch(`${baseUrl}${path}`, {
          ...init,
          headers: this.headers(init.headers),
          cache: 'no-store',
        });

        if (!response.ok) {
          let message = response.statusText;
          let code: string | undefined;
          try {
            const body = (await response.json()) as { error?: string; code?: string };
            message = body.error ?? message;
            code = body.code;
          } catch {
            // Ignore non-JSON error bodies.
          }
          throw new DaemonError(message, response.status, code);
        }

        this.preferredBaseUrl = baseUrl;
        if (response.status === 204) {
          return undefined as T;
        }
        return (await response.json()) as T;
      } catch (error) {
        if (error instanceof DaemonError) {
          throw error;
        }
        errors.push(`${baseUrl}: ${String(error)}`);
      }
    }

    throw new DaemonError(
      `Unable to reach the local daemon. Tried ${orderedBaseUrls.join(', ')}.\n${errors.join('\n')}`,
    );
  }

  private headers(headers?: HeadersInit, json = true): Headers {
    const merged = new Headers(headers);
    if (json) {
      merged.set('Content-Type', 'application/json');
    }
    if (this.token) {
      merged.set('Authorization', `Bearer ${this.token}`);
    }
    return merged;
  }

  private orderedBaseUrls(): string[] {
    if (!this.preferredBaseUrl) {
      return [...this.baseUrls];
    }
    return [
      this.preferredBaseUrl,
      ...this.baseUrls.filter((value) => value !== this.preferredBaseUrl),
    ];
  }
}
