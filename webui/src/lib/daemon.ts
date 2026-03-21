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
