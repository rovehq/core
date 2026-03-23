'use client';

export type AuthState = 'uninitialized' | 'locked' | 'unlocked' | 'reauth_required';
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
}

export interface DaemonConfig {
  node_name: string;
  profile: 'desktop' | 'headless';
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
  configured: boolean;
  token_configured: boolean;
  service_url: string;
  network_id?: string | null;
  managed_name_sync: boolean;
  service_online: boolean;
  joined: boolean;
  node_id?: string | null;
  network_name?: string | null;
  network_status?: string | null;
  assigned_addresses: string[];
  transport_records: RemoteTransportRecord[];
  message?: string | null;
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

export type DaemonEvent =
  | { type: 'auth.locked' }
  | { type: 'daemon.status'; state: string }
  | { type: 'task.created'; task_id: string }
  | { type: 'task.event'; task_id: string; event: unknown }
  | { type: 'task.completed'; task_id: string; result?: string }
  | { type: 'approval.required'; task_id: string; risk: string }
  | { type: 'remote.node.updated'; node_name: string };

const DEFAULT_BASE_URLS = [
  'https://127.0.0.1:47630',
  'http://127.0.0.1:47630',
];

const TOKEN_KEY = 'rove_webui_access_token';

export function readStoredToken(): string | null {
  if (typeof window === 'undefined') {
    return null;
  }
  return window.sessionStorage.getItem(TOKEN_KEY);
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

function configuredBaseUrls(): string[] {
  const configured = process.env.NEXT_PUBLIC_ROVE_DAEMON_URLS
    ?.split(',')
    .map((value) => value.trim())
    .filter(Boolean);
  return configured && configured.length > 0 ? configured : DEFAULT_BASE_URLS;
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
  private readonly baseUrls: string[];

  constructor(token?: string) {
    this.token = token;
    this.baseUrls = configuredBaseUrls();
  }

  setToken(token?: string) {
    this.token = token;
  }

  currentBaseUrl(): string | null {
    return this.preferredBaseUrl ?? this.baseUrls[0] ?? null;
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

  async listServices(): Promise<ServiceStatus[]> {
    return this.request<ServiceStatus[]>('/v1/services');
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

  async listExtensions(): Promise<ExtensionRecord[]> {
    return this.request<ExtensionRecord[]>('/v1/extensions');
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

  async zeroTierJoin(networkId?: string): Promise<ZeroTierStatus> {
    return this.request<ZeroTierStatus>('/v1/remote/transports/zerotier', {
      method: 'POST',
      body: JSON.stringify({ network_id: networkId }),
    });
  }

  async createTask(
    input: string,
    options?: { parallel?: boolean; isolate?: 'none' | 'worktree' | 'snapshot' },
  ): Promise<CreateTaskResponse> {
    return this.request<CreateTaskResponse>('/v1/tasks', {
      method: 'POST',
      body: JSON.stringify({
        input,
        parallel: options?.parallel ?? false,
        isolate: options?.isolate,
      }),
    });
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

  private headers(headers?: HeadersInit): Headers {
    const merged = new Headers(headers);
    merged.set('Content-Type', 'application/json');
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
