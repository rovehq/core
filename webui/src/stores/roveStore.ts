'use client';

import { create } from 'zustand';

import {
  ApprovalRule,
  ApprovalRequest,
  AuthState,
  AuthStatus,
  CatalogExtensionRecord,
  DaemonConfig,
  DaemonError,
  DEFAULT_DAEMON_PORT,
  DaemonHello,
  DaemonEvent,
  DispatchBrainView,
  ExtensionRecord,
  ExtensionUpdateRecord,
  PolicyExplainReport,
  readStoredDaemonPort,
  RemoteDiscoveryCandidate,
  PolicySummary,
  RemotePeer,
  RemoteStatus,
  RoveDaemonClient,
  ServiceInstallStatus,
  ServiceStatus,
  TaskSummary,
  ZeroTierStatus,
  readStoredToken,
  writeStoredToken,
} from '@/lib/daemon';

type AppScreenState =
  | 'checking'
  | 'offline'
  | 'uninitialized'
  | 'locked'
  | 'unlocked'
  | 'reauth_required';

interface WebSocketState {
  connected: boolean;
  connecting: boolean;
  error: string | null;
}

interface TaskRecord {
  id: string;
  input: string;
  status: TaskSummary['status'];
  providerUsed?: string | null;
  durationMs?: number | null;
  createdAt: number;
  completedAt?: number | null;
  latestEvent?: string | null;
}

interface RoveStore {
  appState: AppScreenState;
  hello: DaemonHello | null;
  authStatus: AuthStatus | null;
  config: DaemonConfig | null;
  daemonUrl: string | null;
  daemonPort: number | null;
  token: string | null;
  error: string | null;
  tasks: TaskRecord[];
  services: ServiceStatus[];
  extensions: ExtensionRecord[];
  extensionCatalog: CatalogExtensionRecord[];
  extensionUpdates: ExtensionUpdateRecord[];
  brains: { dispatch: DispatchBrainView } | null;
  policies: PolicySummary[];
  policyExplain: PolicyExplainReport | null;
  remoteStatus: RemoteStatus | null;
  remoteNodes: RemotePeer[];
  remoteCandidates: RemoteDiscoveryCandidate[];
  approvalRules: ApprovalRule[];
  approvals: ApprovalRequest[];
  serviceInstall: ServiceInstallStatus | null;
  zeroTier: ZeroTierStatus | null;
  ws: WebSocketState;
  setDaemonPort: (port: number | null) => Promise<boolean>;
  initialize: () => Promise<void>;
  setupPassword: (password: string, nodeName: string, mode: string) => Promise<boolean>;
  login: (password: string) => Promise<boolean>;
  reauth: (password: string) => Promise<boolean>;
  lock: () => Promise<void>;
  submitTask: (
    input: string,
    options?: { parallel?: boolean; isolate?: 'none' | 'worktree' | 'snapshot'; node?: string },
  ) => Promise<boolean>;
  refreshTasks: () => Promise<void>;
  refreshServices: () => Promise<void>;
  refreshExtensions: () => Promise<void>;
  refreshExtensionCatalog: (force?: boolean) => Promise<void>;
  refreshExtensionUpdates: () => Promise<void>;
  installExtension: (input: {
    kind?: string;
    source: string;
    registry?: string;
    version?: string;
  }) => Promise<boolean>;
  upgradeExtension: (input: {
    kind?: string;
    source: string;
    registry?: string;
    version?: string;
  }) => Promise<boolean>;
  refreshConfig: () => Promise<void>;
  refreshBrains: () => Promise<void>;
  refreshPolicies: () => Promise<void>;
  explainPolicy: (task: string) => Promise<boolean>;
  setPolicyEnabled: (name: string, enabled: boolean) => Promise<boolean>;
  addPolicy: (name: string, scope: 'user' | 'workspace' | 'project') => Promise<boolean>;
  removePolicy: (name: string) => Promise<boolean>;
  refreshRemote: () => Promise<void>;
  trustRemoteNode: (name: string) => Promise<boolean>;
  unpairRemoteNode: (name: string) => Promise<boolean>;
  refreshApprovalRules: () => Promise<void>;
  addApprovalRule: (rule: ApprovalRule) => Promise<boolean>;
  removeApprovalRule: (id: string) => Promise<boolean>;
  refreshApprovals: () => Promise<void>;
  resolveApproval: (id: string, approved: boolean) => Promise<boolean>;
  refreshServiceInstall: () => Promise<void>;
  installService: (mode: 'login' | 'boot', profile?: 'desktop' | 'headless', port?: number) => Promise<boolean>;
  uninstallService: (mode: 'login' | 'boot') => Promise<boolean>;
  refreshZeroTier: () => Promise<void>;
  installZeroTier: () => Promise<boolean>;
  uninstallZeroTier: () => Promise<boolean>;
  setupZeroTier: (input: {
    network_id: string;
    api_token_key?: string;
    managed_name_sync?: boolean;
  }) => Promise<boolean>;
  joinZeroTier: (networkId?: string) => Promise<boolean>;
  trustRemoteCandidate: (candidateId: string) => Promise<boolean>;
  useDispatchBrain: (model: string) => Promise<boolean>;
  setServiceEnabled: (name: string, enabled: boolean) => Promise<boolean>;
  setExtensionEnabled: (kind: string, name: string, enabled: boolean) => Promise<boolean>;
  removeExtension: (kind: string, name: string) => Promise<boolean>;
  updateConfig: (payload: Partial<DaemonConfig>) => Promise<boolean>;
  clearError: () => void;
}

const daemon = new RoveDaemonClient();
let ws: WebSocket | null = null;
let authPollTimer: number | null = null;

function setStoredSession(token: string | null) {
  daemon.setToken(token ?? undefined);
  writeStoredToken(token);
}

function deriveAppState(authState: AuthState, hasToken: boolean): AppScreenState {
  switch (authState) {
    case 'uninitialized':
      return 'uninitialized';
    case 'reauth_required':
      return 'reauth_required';
    case 'unlocked':
      return hasToken ? 'unlocked' : 'locked';
    case 'locked':
    default:
      return 'locked';
  }
}

function mapTask(task: TaskSummary): TaskRecord {
  return {
    id: task.id,
    input: task.input,
    status: task.status,
    providerUsed: task.provider_used,
    durationMs: task.duration_ms,
    createdAt: task.created_at * 1000,
    completedAt: task.completed_at ? task.completed_at * 1000 : null,
  };
}

function stopEventStream() {
  if (ws) {
    ws.onopen = null;
    ws.onmessage = null;
    ws.onerror = null;
    ws.onclose = null;
    ws.close();
  }
  ws = null;
}

function stopAuthPolling() {
  if (authPollTimer !== null && typeof window !== 'undefined') {
    window.clearInterval(authPollTimer);
    authPollTimer = null;
  }
}

function startAuthPolling(get: () => RoveStore) {
  stopAuthPolling();
  if (typeof window === 'undefined') {
    return;
  }

  authPollTimer = window.setInterval(async () => {
    const { token, appState } = get();
    if (!token || (appState !== 'unlocked' && appState !== 'reauth_required')) {
      return;
    }

    try {
      const status = await daemon.authStatus();
      useRoveStore.setState({
        authStatus: status,
        appState: deriveAppState(status.state, true),
      });
    } catch (error) {
      if (error instanceof DaemonError && error.status === 401) {
        stopEventStream();
        setStoredSession(null);
        useRoveStore.setState({
          token: null,
          authStatus: null,
          appState: 'locked',
          ws: { connected: false, connecting: false, error: null },
        });
      }
    }
  }, 30000);
}

function connectEvents(get: () => RoveStore) {
  stopEventStream();

  const { token } = get();
  if (!token) {
    return;
  }

  useRoveStore.setState({
    ws: {
      connected: false,
      connecting: true,
      error: null,
    },
  });

  try {
    ws = daemon.connectEvents((event) => handleEvent(event, get));
  } catch (error) {
    useRoveStore.setState({
      ws: {
        connected: false,
        connecting: false,
        error: error instanceof Error ? error.message : 'Unable to connect to daemon events',
      },
    });
    return;
  }

  ws.onopen = () => {
    useRoveStore.setState({
      ws: { connected: true, connecting: false, error: null },
    });
    ws?.send(JSON.stringify({ type: 'subscribe', topic: 'tasks' }));
    ws?.send(JSON.stringify({ type: 'subscribe', topic: 'daemon' }));
  };

  ws.onclose = () => {
    const { token: currentToken, appState } = get();
    useRoveStore.setState({
      ws: { connected: false, connecting: false, error: null },
    });
    if (currentToken && (appState === 'unlocked' || appState === 'reauth_required')) {
      window.setTimeout(() => connectEvents(get), 3000);
    }
  };

  ws.onerror = () => {
    useRoveStore.setState({
      ws: {
        connected: false,
        connecting: false,
        error: 'Live updates disconnected',
      },
    });
  };
}

function handleEvent(event: DaemonEvent, get: () => RoveStore) {
  switch (event.type) {
    case 'auth.locked':
      stopEventStream();
      setStoredSession(null);
      useRoveStore.setState({
        token: null,
        authStatus: null,
        appState: 'locked',
        ws: { connected: false, connecting: false, error: null },
      });
      return;
    case 'task.created':
      useRoveStore.setState((state) => ({
        tasks: [
          {
            id: event.task_id,
            input: 'Task accepted',
            status: 'pending',
            createdAt: Date.now(),
          },
          ...state.tasks.filter((task) => task.id !== event.task_id),
        ],
      }));
      return;
    case 'task.event':
      useRoveStore.setState((state) => ({
        tasks: state.tasks.map((task) =>
          task.id === event.task_id
            ? {
                ...task,
                status: task.status === 'pending' ? 'running' : task.status,
                latestEvent: summarizeEvent(event.event),
              }
            : task,
        ),
      }));
      return;
    case 'task.completed':
      void get().refreshTasks();
      return;
    case 'daemon.status':
    case 'remote.node.updated':
      void get().initialize();
      return;
    case 'approval.required':
      void get().refreshApprovals();
      useRoveStore.setState({
        error: `Approval required for task ${event.task_id} (${event.risk})`,
      });
      return;
  }
}

function summarizeEvent(event: unknown): string | null {
  if (!event || typeof event !== 'object') {
    return null;
  }
  const record = event as Record<string, unknown>;
  if (typeof record.kind === 'string') {
    return record.kind;
  }
  if (typeof record.event_type === 'string') {
    return record.event_type;
  }
  return null;
}

export const useRoveStore = create<RoveStore>((set, get) => ({
  appState: 'checking',
  hello: null,
  authStatus: null,
  config: null,
  daemonUrl: null,
  daemonPort: DEFAULT_DAEMON_PORT,
  token: null,
  error: null,
  tasks: [],
  services: [],
  extensions: [],
  extensionCatalog: [],
  extensionUpdates: [],
  brains: null,
  policies: [],
  policyExplain: null,
  remoteStatus: null,
  remoteNodes: [],
  remoteCandidates: [],
  approvalRules: [],
  approvals: [],
  serviceInstall: null,
  zeroTier: null,
  ws: { connected: false, connecting: false, error: null },

  initialize: async () => {
    const storedToken = readStoredToken();
    daemon.setPortOverride(readStoredDaemonPort());
    setStoredSession(storedToken);

    try {
      const hello = await daemon.hello();
      const daemonUrl = daemon.currentBaseUrl();
      const daemonPort = daemon.currentPort() ?? DEFAULT_DAEMON_PORT;
      const nextState = deriveAppState(hello.auth_state, Boolean(storedToken));

      set({
        hello,
        daemonUrl,
        daemonPort,
        token: storedToken,
        appState: nextState,
        error: null,
      });

      if (storedToken && (nextState === 'unlocked' || nextState === 'reauth_required')) {
        const authStatus = await daemon.authStatus();
        const [services, extensions, extensionCatalog, extensionUpdates, config, brains, policies, remoteStatus, remoteNodes, remoteCandidates, approvals, approvalRules, serviceInstall, zeroTier] = await Promise.all([
          daemon.listServices(),
          daemon.listExtensions(),
          daemon.listExtensionCatalog(),
          daemon.listExtensionUpdates(),
          daemon.getConfig(),
          daemon.listBrains(),
          daemon.listPolicies(),
          daemon.remoteStatus(),
          daemon.listRemoteNodes(),
          daemon.listRemoteDiscovery(),
          daemon.listApprovals(),
          daemon.listApprovalRules(),
          daemon.serviceInstallStatus(),
          daemon.zeroTierStatus(),
        ]);
        set({
          authStatus,
          services,
          extensions,
          extensionCatalog,
          extensionUpdates,
          config,
          brains,
          policies,
          remoteStatus,
          remoteNodes,
          remoteCandidates,
          approvals,
          approvalRules: approvalRules.rules,
          serviceInstall,
          zeroTier,
          daemonPort,
          appState: deriveAppState(authStatus.state, true),
        });
        await get().refreshTasks();
        connectEvents(get);
        startAuthPolling(get);
      } else {
        stopEventStream();
        stopAuthPolling();
        set({
          authStatus: null,
          config: null,
          services: [],
          extensions: [],
          extensionCatalog: [],
          extensionUpdates: [],
          brains: null,
          policies: [],
          policyExplain: null,
          remoteStatus: null,
          remoteNodes: [],
          remoteCandidates: [],
          approvalRules: [],
          approvals: [],
          serviceInstall: null,
          zeroTier: null,
          tasks: nextState === 'locked' || nextState === 'uninitialized' ? [] : get().tasks,
          ws: { connected: false, connecting: false, error: null },
        });
      }
    } catch (error) {
      stopEventStream();
      stopAuthPolling();
      setStoredSession(null);
      set({
        daemonUrl: daemon.currentBaseUrl(),
        daemonPort: daemon.currentPort() ?? DEFAULT_DAEMON_PORT,
        token: null,
        authStatus: null,
        config: null,
        services: [],
        extensions: [],
        extensionCatalog: [],
        extensionUpdates: [],
        brains: null,
        policies: [],
        policyExplain: null,
        remoteStatus: null,
        remoteNodes: [],
        remoteCandidates: [],
        approvalRules: [],
        approvals: [],
        serviceInstall: null,
        zeroTier: null,
        appState: 'offline',
        error: error instanceof Error ? error.message : 'Unable to reach daemon',
        ws: { connected: false, connecting: false, error: null },
      });
    }
  },

  setDaemonPort: async (port) => {
    try {
      daemon.setPortOverride(port);
      stopEventStream();
      stopAuthPolling();
      set({
        daemonUrl: daemon.currentBaseUrl(),
        daemonPort: daemon.currentPort() ?? DEFAULT_DAEMON_PORT,
        error: null,
        appState: 'checking',
      });
      await get().initialize();
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to update daemon port' });
      return false;
    }
  },

  setupPassword: async (password, nodeName, mode) => {
    try {
      const session = await daemon.authSetup(password, nodeName, mode);
      setStoredSession(session.access_token);
      set({ token: session.access_token });
      await get().initialize();
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Setup failed' });
      return false;
    }
  },

  login: async (password) => {
    try {
      const session = await daemon.authLogin(password);
      setStoredSession(session.access_token);
      set({ token: session.access_token });
      await get().initialize();
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Login failed' });
      return false;
    }
  },

  reauth: async (password) => {
    try {
      const status = await daemon.authReauth(password);
      set({
        authStatus: status,
        appState: deriveAppState(status.state, Boolean(get().token)),
        error: null,
      });
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Reauthentication failed' });
      return false;
    }
  },

  lock: async () => {
    const { token } = get();
    if (!token) {
      return;
    }

    try {
      await daemon.authLock();
    } finally {
      stopEventStream();
      stopAuthPolling();
      setStoredSession(null);
      set({
        token: null,
        authStatus: null,
        config: null,
        services: [],
        extensions: [],
        extensionCatalog: [],
        extensionUpdates: [],
        brains: null,
        policies: [],
        policyExplain: null,
        remoteStatus: null,
        remoteNodes: [],
        remoteCandidates: [],
        approvalRules: [],
        approvals: [],
        serviceInstall: null,
        zeroTier: null,
        appState: 'locked',
        tasks: [],
        ws: { connected: false, connecting: false, error: null },
      });
    }
  },

  submitTask: async (input, options) => {
    try {
      const accepted = await daemon.createTask(input, options);
      set((state) => ({
        tasks: [
          {
            id: accepted.task_id,
            input,
            status: 'pending',
            createdAt: Date.now(),
          },
          ...state.tasks.filter((task) => task.id !== accepted.task_id),
        ],
        error: null,
      }));
      return true;
    } catch (error) {
      if (error instanceof DaemonError && error.status === 401) {
        await get().initialize();
      }
      set({ error: error instanceof Error ? error.message : 'Task submission failed' });
      return false;
    }
  },

  refreshTasks: async () => {
    try {
      const tasks = await daemon.listTasks();
      set({
        tasks: tasks.map(mapTask),
        error: null,
      });
    } catch (error) {
      if (error instanceof DaemonError && error.status === 401) {
        await get().initialize();
        return;
      }
      set({ error: error instanceof Error ? error.message : 'Unable to load task history' });
    }
  },

  refreshServices: async () => {
    try {
      const services = await daemon.listServices();
      set({ services, error: null });
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to load services' });
    }
  },

  refreshExtensions: async () => {
    try {
      const extensions = await daemon.listExtensions();
      set({ extensions, error: null });
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to load extensions' });
    }
  },

  refreshExtensionCatalog: async (force = false) => {
    try {
      const extensionCatalog = force
        ? await daemon.refreshExtensionCatalog()
        : await daemon.listExtensionCatalog();
      set({ extensionCatalog, error: null });
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to load extension catalog' });
    }
  },

  refreshExtensionUpdates: async () => {
    try {
      const extensionUpdates = await daemon.listExtensionUpdates();
      set({ extensionUpdates, error: null });
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to load extension updates' });
    }
  },

  installExtension: async (input) => {
    try {
      await daemon.installExtension(input);
      const [extensions, extensionCatalog, extensionUpdates] = await Promise.all([
        daemon.listExtensions(),
        daemon.listExtensionCatalog(),
        daemon.listExtensionUpdates(),
      ]);
      set({ extensions, extensionCatalog, extensionUpdates, error: null });
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to install extension' });
      return false;
    }
  },

  upgradeExtension: async (input) => {
    try {
      await daemon.upgradeExtension(input);
      const [extensions, extensionCatalog, extensionUpdates] = await Promise.all([
        daemon.listExtensions(),
        daemon.listExtensionCatalog(),
        daemon.listExtensionUpdates(),
      ]);
      set({ extensions, extensionCatalog, extensionUpdates, error: null });
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to upgrade extension' });
      return false;
    }
  },

  refreshConfig: async () => {
    try {
      const config = await daemon.getConfig();
      set({ config, error: null });
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to load daemon config' });
    }
  },

  refreshBrains: async () => {
    try {
      const brains = await daemon.listBrains();
      set({ brains, error: null });
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to load brains' });
    }
  },

  refreshPolicies: async () => {
    try {
      const policies = await daemon.listPolicies();
      set({ policies, error: null });
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to load policies' });
    }
  },

  explainPolicy: async (task) => {
    try {
      const policyExplain = await daemon.explainPolicy(task);
      set({ policyExplain, error: null });
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to explain policy' });
      return false;
    }
  },

  setPolicyEnabled: async (name, enabled) => {
    try {
      await daemon.setPolicyEnabled(name, enabled);
      await get().refreshPolicies();
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to update policy state' });
      return false;
    }
  },

  addPolicy: async (name, scope) => {
    try {
      await daemon.addPolicy(name, scope);
      await get().refreshPolicies();
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to create policy' });
      return false;
    }
  },

  removePolicy: async (name) => {
    try {
      await daemon.removePolicy(name);
      await get().refreshPolicies();
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to remove policy' });
      return false;
    }
  },

  refreshRemote: async () => {
    try {
      const [remoteStatus, remoteNodes, remoteCandidates] = await Promise.all([
        daemon.remoteStatus(),
        daemon.listRemoteNodes(),
        daemon.listRemoteDiscovery(),
      ]);
      set({ remoteStatus, remoteNodes, remoteCandidates, error: null });
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to load remote state' });
    }
  },

  trustRemoteNode: async (name) => {
    try {
      await daemon.trustRemoteNode(name);
      await get().refreshRemote();
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to trust remote node' });
      return false;
    }
  },

  unpairRemoteNode: async (name) => {
    try {
      await daemon.unpairRemoteNode(name);
      await get().refreshRemote();
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to unpair remote node' });
      return false;
    }
  },

  refreshApprovalRules: async () => {
    try {
      const file = await daemon.listApprovalRules();
      set({ approvalRules: file.rules, error: null });
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to load approval rules' });
    }
  },

  addApprovalRule: async (rule) => {
    try {
      const file = await daemon.addApprovalRule(rule);
      set({ approvalRules: file.rules, error: null });
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to save approval rule' });
      return false;
    }
  },

  removeApprovalRule: async (id) => {
    try {
      await daemon.removeApprovalRule(id);
      await get().refreshApprovalRules();
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to remove approval rule' });
      return false;
    }
  },

  refreshApprovals: async () => {
    try {
      const approvals = await daemon.listApprovals();
      set({ approvals, error: null });
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to load approvals' });
    }
  },

  resolveApproval: async (id, approved) => {
    try {
      await daemon.resolveApproval(id, approved);
      await get().refreshApprovals();
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to resolve approval' });
      return false;
    }
  },

  refreshServiceInstall: async () => {
    try {
      const serviceInstall = await daemon.serviceInstallStatus();
      set({ serviceInstall, error: null });
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to load service install state' });
    }
  },

  installService: async (mode, profile, port) => {
    try {
      await daemon.installService(mode, profile, port);
      await get().refreshServiceInstall();
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to install daemon service' });
      return false;
    }
  },

  uninstallService: async (mode) => {
    try {
      await daemon.uninstallService(mode);
      await get().refreshServiceInstall();
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to uninstall daemon service' });
      return false;
    }
  },

  refreshZeroTier: async () => {
    try {
      const [zeroTier, remoteCandidates] = await Promise.all([
        daemon.zeroTierStatus(),
        daemon.listRemoteDiscovery(),
      ]);
      set({ zeroTier, remoteCandidates, error: null });
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to load ZeroTier status' });
    }
  },

  installZeroTier: async () => {
    try {
      const zeroTier = await daemon.zeroTierInstall();
      set({ zeroTier, error: null });
      await get().refreshRemote();
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to install ZeroTier transport' });
      return false;
    }
  },

  uninstallZeroTier: async () => {
    try {
      const zeroTier = await daemon.zeroTierUninstall();
      set({ zeroTier, remoteCandidates: [], error: null });
      await get().refreshRemote();
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to disable ZeroTier transport' });
      return false;
    }
  },

  setupZeroTier: async (input) => {
    try {
      const zeroTier = await daemon.zeroTierSetup(input);
      set({ zeroTier, error: null });
      await get().refreshRemote();
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to configure ZeroTier transport' });
      return false;
    }
  },

  joinZeroTier: async (networkId) => {
    try {
      const zeroTier = await daemon.zeroTierJoin(networkId);
      set({ zeroTier, error: null });
      await get().refreshRemote();
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to join ZeroTier network' });
      return false;
    }
  },

  trustRemoteCandidate: async (candidateId) => {
    try {
      await daemon.trustRemoteCandidate(candidateId);
      await get().refreshRemote();
      await get().refreshZeroTier();
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to trust ZeroTier candidate' });
      return false;
    }
  },

  useDispatchBrain: async (model) => {
    try {
      const dispatch = await daemon.useDispatchBrain(model);
      set((state) => ({ brains: { ...(state.brains ?? { dispatch }), dispatch }, error: null }));
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to switch dispatch brain' });
      return false;
    }
  },

  setServiceEnabled: async (name, enabled) => {
    try {
      const updated = await daemon.setServiceEnabled(name, enabled);
      set((state) => ({
        services: state.services.map((service) =>
          service.name === updated.name ? updated : service,
        ),
        error: null,
      }));
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to update service' });
      return false;
    }
  },

  setExtensionEnabled: async (kind, name, enabled) => {
    try {
      await daemon.setExtensionEnabled(kind, name, enabled);
      const [extensions, extensionCatalog, extensionUpdates] = await Promise.all([
        daemon.listExtensions(),
        daemon.listExtensionCatalog(),
        daemon.listExtensionUpdates(),
      ]);
      set({ extensions, extensionCatalog, extensionUpdates, error: null });
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to update extension' });
      return false;
    }
  },

  removeExtension: async (kind, name) => {
    try {
      await daemon.removeExtension(kind, name);
      const [extensions, extensionCatalog, extensionUpdates] = await Promise.all([
        daemon.listExtensions(),
        daemon.listExtensionCatalog(),
        daemon.listExtensionUpdates(),
      ]);
      set({ extensions, extensionCatalog, extensionUpdates, error: null });
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to remove extension' });
      return false;
    }
  },

  updateConfig: async (payload) => {
    try {
      const config = await daemon.updateConfig(payload);
      set({ config, error: null });
      await get().initialize();
      return true;
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Unable to save daemon config' });
      return false;
    }
  },

  clearError: () => set({ error: null }),
}));
