'use client';

import { useEffect, useState } from 'react';
import Nav from '@/components/Nav';
import {
  DEFAULT_DAEMON_PORT,
  OverviewResponse,
  readStoredToken,
  RoveDaemonClient,
} from '@/lib/daemon';
import { useRoveStore } from '@/stores/roveStore';

export default function MessagesPage() {
  const {
    appState,
    authStatus,
    clearError,
    daemonPort,
    daemonUrl,
    error,
    hello,
    initialize,
    lock,
    login,
    reauth,
    refreshTasks,
    setupPassword,
    setDaemonPort,
    submitTask,
    tasks,
    ws,
  } = useRoveStore();
  const [input, setInput] = useState('');
  const [password, setPassword] = useState('');
  const [nodeName, setNodeName] = useState('my-device');
  const [mode, setMode] = useState('local_only');
  const [portInput, setPortInput] = useState(String(DEFAULT_DAEMON_PORT));
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [overview, setOverview] = useState<OverviewResponse | null>(null);
  const [overviewError, setOverviewError] = useState<string | null>(null);

  useEffect(() => {
    void initialize();
  }, [initialize]);

  useEffect(() => {
    setPortInput(String(daemonPort ?? DEFAULT_DAEMON_PORT));
  }, [daemonPort]);

  useEffect(() => {
    if (appState === 'unlocked') {
      void refreshOverview();
    }
  }, [appState]);

  const handleSubmit = async (e?: React.FormEvent) => {
    e?.preventDefault();
    if (!input.trim() || isSubmitting) return;

    setIsSubmitting(true);
    if (await submitTask(input.trim())) {
      setInput('');
      void refreshOverview();
    }
    setIsSubmitting(false);
  };

  async function refreshOverview() {
    try {
      setOverviewError(null);
      setOverview(await daemonClient().getOverview());
    } catch (nextError) {
      setOverviewError(nextError instanceof Error ? nextError.message : String(nextError));
    }
  }

  async function convertTask(kind: 'agent' | 'workflow', taskId: string) {
    try {
      if (kind === 'agent') {
        await daemonClient().createAgentFromTask(taskId);
      } else {
        await daemonClient().createWorkflowFromTask(taskId);
      }
      await refreshOverview();
    } catch (nextError) {
      setOverviewError(nextError instanceof Error ? nextError.message : String(nextError));
    }
  }

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && e.ctrlKey) {
      void handleSubmit();
    }
  };

  const getStatusColor = (status: string) => {
    switch (status) {
      case 'pending': return 'bg-warning/20 text-warning';
      case 'running': return 'bg-primary/20 text-primary';
      case 'completed': return 'bg-success/20 text-success';
      case 'failed': return 'bg-error/20 text-error';
      default: return 'bg-gray-500/20 text-gray-400';
    }
  };

  const formatDuration = (ms?: number) => {
    if (!ms) return '';
    if (ms < 1000) return `${ms}ms`;
    return `${(ms / 1000).toFixed(1)}s`;
  };

  const recentTasks = overview?.tasks ?? tasks.map((task) => ({
    id: task.id,
    input: task.input,
    status: task.status,
    provider_used: task.providerUsed,
    duration_ms: task.durationMs,
    created_at: task.createdAt,
    completed_at: task.completedAt,
  }));

  if (appState === 'checking') {
    return <FullScreenMessage title="Connecting to Rove" body="Probing the local daemon and restoring your session." />;
  }

  if (appState === 'offline') {
    return (
      <AuthShell title="Local Daemon Not Reachable" subtitle="Start the daemon from the Rove menu bar app, then refresh.">
        <div className="space-y-4">
          <p className="text-sm text-gray-400 whitespace-pre-wrap">
            {error ?? `The browser could not reach your local daemon. Default probe port is ${DEFAULT_DAEMON_PORT}.`}
          </p>
          <Field label="Daemon port">
            <input
              value={portInput}
              onChange={(event) => setPortInput(event.target.value)}
              className="w-full rounded-lg border border-surface2 bg-background px-3 py-3 outline-none focus:border-primary"
              placeholder={String(DEFAULT_DAEMON_PORT)}
              inputMode="numeric"
            />
          </Field>
          <div className="grid gap-3 sm:grid-cols-2">
            <button
              onClick={() => void setDaemonPort(portInput.trim() ? Number(portInput) : null)}
              className="w-full rounded-lg bg-primary px-4 py-3 font-medium hover:bg-primary/80"
            >
              Save Port And Retry
            </button>
            <button
              onClick={() => {
                setPortInput(String(DEFAULT_DAEMON_PORT));
                void setDaemonPort(null);
              }}
              className="w-full rounded-lg border border-surface2 px-4 py-3 font-medium hover:border-primary"
            >
              Use Default Probe List
            </button>
          </div>
        </div>
      </AuthShell>
    );
  }

  if (appState === 'uninitialized') {
    return (
      <AuthShell title="Set Up Local Access" subtitle="Create the daemon password that protects your local control plane.">
        <form
          className="space-y-4"
          onSubmit={async (event) => {
            event.preventDefault();
            setIsSubmitting(true);
            await setupPassword(password, nodeName, mode);
            setIsSubmitting(false);
          }}
        >
          <Field label="Node name">
            <input
              value={nodeName}
              onChange={(event) => setNodeName(event.target.value)}
              className="w-full rounded-lg border border-surface2 bg-background px-3 py-3 outline-none focus:border-primary"
              placeholder="office-mac"
            />
          </Field>
          <Field label="Admin password">
            <input
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              className="w-full rounded-lg border border-surface2 bg-background px-3 py-3 outline-none focus:border-primary"
              placeholder="At least 8 characters"
            />
          </Field>
          <Field label="Privacy mode">
            <select
              value={mode}
              onChange={(event) => setMode(event.target.value)}
              className="w-full rounded-lg border border-surface2 bg-background px-3 py-3 outline-none focus:border-primary"
            >
              <option value="local_only">Local only</option>
              <option value="hybrid">Hybrid</option>
              <option value="cloud_enabled">Cloud enabled</option>
            </select>
          </Field>
          <ErrorBanner error={error} onDismiss={clearError} />
          <button
            type="submit"
            disabled={isSubmitting || password.trim().length < 8}
            className="w-full rounded-lg bg-primary px-4 py-3 font-medium hover:bg-primary/80 disabled:bg-surface2 disabled:text-gray-500"
          >
            {isSubmitting ? 'Setting up...' : 'Create Password'}
          </button>
        </form>
      </AuthShell>
    );
  }

  if (appState === 'locked' || appState === 'reauth_required') {
    return (
      <AuthShell
        title={appState === 'reauth_required' ? 'Reauthenticate' : 'Unlock Rove'}
        subtitle={
          appState === 'reauth_required'
            ? 'Sensitive settings need a fresh password check.'
            : 'Enter your local daemon password to continue.'
        }
      >
        <form
          className="space-y-4"
          onSubmit={async (event) => {
            event.preventDefault();
            setIsSubmitting(true);
            if (appState === 'reauth_required') {
              await reauth(password);
            } else {
              await login(password);
            }
            setPassword('');
            setIsSubmitting(false);
          }}
        >
          <Field label="Password">
            <input
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              className="w-full rounded-lg border border-surface2 bg-background px-3 py-3 outline-none focus:border-primary"
              placeholder="Local daemon password"
            />
          </Field>
          <ErrorBanner error={error} onDismiss={clearError} />
          <button
            type="submit"
            disabled={isSubmitting || password.length === 0}
            className="w-full rounded-lg bg-primary px-4 py-3 font-medium hover:bg-primary/80 disabled:bg-surface2 disabled:text-gray-500"
          >
            {isSubmitting ? 'Unlocking...' : appState === 'reauth_required' ? 'Confirm Password' : 'Unlock'}
          </button>
        </form>
      </AuthShell>
    );
  }

  return (
    <div className="min-h-screen flex flex-col">
      <header className="sticky top-0 z-10 bg-background/80 backdrop-blur border-b border-surface2">
        <div className="mx-auto max-w-6xl px-4 py-4">
          <div className="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
            <div>
              <div className="flex items-center gap-3">
                <span className="text-2xl">⌘</span>
                <div>
                  <h1 className="text-2xl font-semibold">Rove</h1>
                  <p className="text-sm text-gray-400">
                    {hello?.node.node_name ?? 'local-node'} · {hello?.node.role === 'executor_only' ? 'executor-only node' : 'full node'}
                  </p>
                </div>
              </div>
            </div>

            <div className="flex flex-wrap items-center gap-3">
              <StatusPill tone={ws.connected ? 'success' : ws.connecting ? 'warning' : 'error'}>
                {ws.connected ? 'Live connected' : ws.connecting ? 'Connecting stream' : 'Stream offline'}
              </StatusPill>
              <StatusPill tone="default">{daemonUrl ?? 'daemon unknown'}</StatusPill>
              <StatusPill tone="default">
                idle {formatSeconds(authStatus?.idle_expires_in_secs ?? null)}
              </StatusPill>
              <button
                onClick={() => void lock()}
                className="rounded-lg border border-surface2 px-4 py-2 text-sm hover:border-primary hover:text-white"
              >
                Lock
              </button>
            </div>
          </div>

          <div className="mt-4">
            <Nav />
          </div>
        </div>
      </header>

      <main className="mx-auto flex w-full max-w-6xl flex-1 flex-col gap-6 px-4 py-6">
        <div className="grid gap-4 md:grid-cols-3">
          <SummaryCard label="Brains" value={hello?.capabilities.brains.join(', ') || 'dispatch'} />
          <SummaryCard label="Agents" value={String(overview?.counts.agents ?? 0)} />
          <SummaryCard label="Workflows" value={String(overview?.counts.workflows ?? 0)} />
        </div>

        <section className="rounded-xl border border-surface2 bg-surface p-4">
          <div className="mb-3 flex items-center justify-between">
            <div>
              <h2 className="text-lg font-semibold">Run a Task</h2>
              <p className="text-sm text-gray-400">The hosted UI submits work to your local daemon over the localhost control plane.</p>
            </div>
            <button
              onClick={() => void refreshTasks()}
              className="rounded-lg border border-surface2 px-4 py-2 text-sm hover:border-primary"
            >
              Refresh
            </button>
          </div>

          <form onSubmit={(event) => void handleSubmit(event)}>
            <div className="flex flex-col gap-3 lg:flex-row">
              <textarea
                value={input}
                onChange={(event) => setInput(event.target.value)}
                onKeyDown={handleKeyDown}
                placeholder="Find the latest failure in this workspace, summarize it, and suggest the next safe fix."
                className="min-h-[90px] flex-1 resize-none rounded-lg border border-surface2 bg-background px-4 py-3 outline-none focus:border-primary"
                rows={4}
                disabled={isSubmitting}
              />
              <button
                type="submit"
                disabled={isSubmitting || !input.trim()}
                className="rounded-lg bg-primary px-6 py-3 font-medium hover:bg-primary/80 disabled:bg-surface2 disabled:text-gray-500"
              >
                {isSubmitting ? 'Submitting...' : 'Run Task'}
              </button>
            </div>
          </form>
          <p className="mt-2 text-sm text-gray-500">
            Press <kbd className="rounded bg-surface2 px-2 py-0.5">Ctrl</kbd> + <kbd className="rounded bg-surface2 px-2 py-0.5">Enter</kbd> to send.
          </p>
          <ErrorBanner error={error} onDismiss={clearError} />
        </section>

        <section className="grid gap-4 xl:grid-cols-2">
          <DashboardPanel
            title="Health"
            subtitle="First-run truth for config, data, database, and service install state."
            actionLabel="Refresh"
            onAction={() => void refreshOverview()}
          >
            {overview?.health ? (
              <div className="space-y-3 text-sm">
                <div className="rounded-lg border border-surface p-3">
                  <div className="flex items-center justify-between gap-3">
                    <div className="font-medium">
                      {overview.health.healthy ? 'Ready' : 'Needs attention'}
                    </div>
                    <span className={`rounded-full px-2 py-0.5 text-xs ${overview.health.healthy ? 'bg-success/10 text-success' : 'bg-warning/10 text-warning'}`}>
                      {overview.health.profile}
                    </span>
                  </div>
                  <div className="mt-2 text-gray-400">
                    {overview.health.node_name} · secret backend {overview.health.secret_backend}
                  </div>
                </div>

                <div className="grid gap-2 md:grid-cols-2">
                  <HealthPathRow label="Config" status={overview.health.config_file} />
                  <HealthPathRow label="Workspace" status={overview.health.workspace} />
                  <HealthPathRow label="Data" status={overview.health.data_dir} />
                  <HealthPathRow label="Database" status={overview.health.database} />
                </div>

                <div className="rounded-lg border border-surface p-3">
                  <div className="font-medium">Service install</div>
                  <div className="mt-2 text-gray-400">
                    login: {overview.health.service_install.login.supported ? (overview.health.service_install.login.installed ? 'installed' : 'not installed') : 'unsupported'}
                    {' · '}
                    boot: {overview.health.service_install.boot.supported ? (overview.health.service_install.boot.installed ? 'installed' : 'not installed') : 'unsupported'}
                  </div>
                </div>

                {overview.health.issues.length ? (
                  <div className="rounded-lg border border-warning/30 bg-warning/5 p-3">
                    <div className="font-medium text-warning">Open issues</div>
                    <div className="mt-2 space-y-1 text-gray-300">
                      {overview.health.issues.slice(0, 4).map((issue) => (
                        <div key={issue}>{issue}</div>
                      ))}
                    </div>
                  </div>
                ) : (
                  <EmptyState text="No initialization or runtime truth issues detected." />
                )}
              </div>
            ) : (
              <EmptyState text="Health snapshot unavailable." />
            )}
          </DashboardPanel>

          <DashboardPanel
            title="Approvals"
            subtitle="Pending approvals and current control-plane channels."
            actionLabel="Refresh"
            onAction={() => void refreshOverview()}
          >
            {overview?.approvals?.length ? (
              <div className="space-y-3">
                {overview.approvals.slice(0, 6).map((approval) => (
                  <div key={approval.id} className="rounded-lg border border-surface p-3 text-sm">
                    <div className="font-medium">{approval.summary}</div>
                    <div className="mt-1 text-gray-500">{approval.id}</div>
                  </div>
                ))}
              </div>
            ) : (
              <EmptyState text="No pending approvals." />
            )}
          </DashboardPanel>

          <DashboardPanel
            title="Channels"
            subtitle="Runtime channel health, bindings, and setup truth."
            actionLabel="Open"
            onAction={() => (window.location.href = '/channels')}
          >
            {overview?.channels?.length ? (
              <div className="space-y-3">
                {overview.channels.map((channel) => (
                  <div key={channel.name} className="rounded-lg border border-surface p-3 text-sm">
                    <div className="flex items-center justify-between gap-3">
                      <div className="font-medium">{channel.name}</div>
                      <span className={`rounded-full px-2 py-0.5 text-xs ${channel.healthy ? 'bg-success/10 text-success' : 'bg-warning/10 text-warning'}`}>
                        {channel.enabled ? 'enabled' : 'disabled'}
                      </span>
                    </div>
                    <p className="mt-2 text-gray-400">{channel.summary}</p>
                  </div>
                ))}
              </div>
            ) : (
              <EmptyState text="No channels configured." />
            )}
          </DashboardPanel>

          <DashboardPanel
            title="Services"
            subtitle="Login/boot service state and runtime surfaces."
          >
            {overview?.services?.length ? (
              <div className="space-y-3">
                {overview.services.map((service) => (
                  <div key={service.name} className="rounded-lg border border-surface p-3 text-sm">
                    <div className="flex items-center justify-between gap-3">
                      <div className="font-medium">{service.name}</div>
                      <span className={`rounded-full px-2 py-0.5 text-xs ${service.enabled ? 'bg-success/10 text-success' : 'bg-surface2 text-gray-400'}`}>
                        {service.enabled ? 'enabled' : 'disabled'}
                      </span>
                    </div>
                  </div>
                ))}
              </div>
            ) : (
              <EmptyState text="No managed services found." />
            )}
          </DashboardPanel>

          <DashboardPanel
            title="Remote"
            subtitle="Current node identity and mesh status."
            actionLabel="Open"
            onAction={() => (window.location.href = '/remote')}
          >
            {overview?.remote ? (
              <div className="space-y-3 text-sm">
                <div className="rounded-lg border border-surface p-3">
                  <div className="font-medium">{overview.remote.node.node_name}</div>
                  <div className="mt-1 text-gray-400">{overview.remote.node.node_id}</div>
                  <div className="mt-2 text-gray-500">
                    {overview.remote.paired_nodes} paired nodes · {overview.remote.transports.length} transports
                  </div>
                </div>
              </div>
            ) : (
              <EmptyState text="Remote status unavailable." />
            )}
          </DashboardPanel>

          <DashboardPanel
            title="Runs"
            subtitle="Recent agent and workflow runs."
          >
            <div className="space-y-3">
              {overview?.agent_runs?.slice(0, 4).map((run) => (
                <div key={run.run_id} className="rounded-lg border border-surface p-3 text-sm">
                  <div className="font-medium">agent:{run.agent_id}</div>
                  <div className="mt-1 text-gray-400">{run.status}</div>
                  <div className="mt-2 text-gray-500">{run.input}</div>
                </div>
              ))}
              {overview?.workflow_runs?.slice(0, 4).map((run) => (
                <div key={run.run_id} className="rounded-lg border border-surface p-3 text-sm">
                  <div className="font-medium">workflow:{run.workflow_id}</div>
                  <div className="mt-1 text-gray-400">{run.status}</div>
                  <div className="mt-2 text-gray-500">{run.input}</div>
                </div>
              ))}
              {!overview?.agent_runs?.length && !overview?.workflow_runs?.length ? (
                <EmptyState text="No agent or workflow runs yet." />
              ) : null}
            </div>
          </DashboardPanel>

          <DashboardPanel
            title="Recent Logs"
            subtitle="Bounded daemon tail for quick operator diagnosis."
          >
            {overview?.recent_logs?.length ? (
              <pre className="max-h-80 overflow-auto rounded-lg border border-surface bg-background/40 p-3 text-xs text-gray-300">
                {overview.recent_logs.join('\n')}
              </pre>
            ) : (
              <EmptyState text="No recent daemon logs yet." />
            )}
          </DashboardPanel>
        </section>

        <section className="overflow-hidden rounded-xl border border-surface2 bg-surface">
          <div className="border-b border-surface2 p-4">
            <h2 className="text-lg font-semibold">Recent Tasks</h2>
            {overviewError ? <p className="mt-2 text-sm text-error whitespace-pre-wrap">{overviewError}</p> : null}
          </div>

          <div className="max-h-[540px] space-y-4 overflow-y-auto p-4">
            {recentTasks.length === 0 ? (
              <div className="rounded-xl border border-dashed border-surface2 p-8 text-center text-gray-500">
                No tasks yet. Start from the form above after unlocking the daemon.
              </div>
            ) : (
              recentTasks.map((task) => (
                <TaskCard
                  key={task.id}
                  task={{
                    id: task.id,
                    input: task.input,
                    status: task.status,
                    providerUsed: task.provider_used,
                    durationMs: task.duration_ms,
                    createdAt: task.created_at,
                    completedAt: task.completed_at,
                  }}
                  getStatusColor={getStatusColor}
                  formatDuration={formatDuration}
                  actions={
                    <div className="mt-3 flex gap-2">
                      <button
                        onClick={() => void convertTask('agent', task.id)}
                        className="rounded border border-surface px-2 py-1 text-xs hover:border-primary"
                      >
                        Convert To Agent
                      </button>
                      <button
                        onClick={() => void convertTask('workflow', task.id)}
                        className="rounded border border-surface px-2 py-1 text-xs hover:border-primary"
                      >
                        Convert To Workflow
                      </button>
                    </div>
                  }
                />
              ))
            )}
          </div>
        </section>
      </main>

      <footer className="border-t border-surface2 py-4 text-center text-sm text-gray-500">
        Hosted UI shell · local daemon authority · March 21, 2026
      </footer>
    </div>
  );
}

function TaskCard({ task, getStatusColor, formatDuration, actions }: {
  task: {
    id: string;
    input: string;
    status: string;
    providerUsed?: string | null;
    durationMs?: number | null;
    createdAt: number;
    completedAt?: number | null;
    latestEvent?: string | null;
  };
  getStatusColor: (s: string) => string;
  formatDuration: (ms?: number) => string;
  actions?: React.ReactNode;
}) {
  return (
    <div className="bg-surface2 rounded-lg p-4 animate-in slide-in-from-top-2">
      <div className="flex items-start justify-between mb-3">
        <div className="flex items-center gap-2">
          <code className="text-xs text-gray-500 font-mono">{task.id.slice(0, 8)}...</code>
          <span className={`px-2 py-0.5 rounded-full text-xs font-medium ${getStatusColor(task.status)}`}>
            {task.status}
          </span>
        </div>
        <span className="text-xs text-gray-500">
          {new Date(normalizeEpochMillis(task.createdAt)).toLocaleTimeString()}
        </span>
      </div>
      
      {task.input && (
        <p className="text-gray-300 mb-3">{task.input}</p>
      )}
      
      {task.status === 'running' && (
        <div className="space-y-2">
          <div className="h-1 bg-surface rounded-full overflow-hidden">
            <div className="h-full bg-gradient-to-r from-primary to-purple-500 animate-pulse w-1/2" />
          </div>
          {task.latestEvent && (
            <p className="text-sm text-gray-400">{task.latestEvent}</p>
          )}
        </div>
      )}
      
      {task.status === 'completed' && (
        <div className="space-y-3">
          <p className="text-gray-400">
            {task.providerUsed ? `Completed using ${task.providerUsed}.` : 'Completed successfully.'}
          </p>
          {task.durationMs && (
            <div className="flex gap-4 text-xs text-gray-500 pt-3 border-t border-surface">
              <span>⏱ {formatDuration(task.durationMs)}</span>
            </div>
          )}
        </div>
      )}
      
      {task.status === 'failed' && (
        <p className="text-error">This task failed. Open history or replay tooling next.</p>
      )}

      {actions}
    </div>
  );
}

function AuthShell({
  title,
  subtitle,
  children,
}: {
  title: string;
  subtitle: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex min-h-screen items-center justify-center bg-background px-4 py-12">
      <div className="w-full max-w-md rounded-2xl border border-surface2 bg-surface p-8 shadow-2xl shadow-black/30">
        <p className="mb-3 text-sm uppercase tracking-[0.3em] text-gray-500">Rove Local</p>
        <h1 className="text-3xl font-semibold">{title}</h1>
        <p className="mt-3 text-sm text-gray-400">{subtitle}</p>
        <div className="mt-8">{children}</div>
      </div>
    </div>
  );
}

function HealthPathRow({
  label,
  status,
}: {
  label: string;
  status: {
    path: string;
    exists: boolean;
    writable: boolean;
  };
}) {
  const tone = status.exists && status.writable
    ? 'bg-success/10 text-success'
    : 'bg-warning/10 text-warning';

  return (
    <div className="rounded-lg border border-surface p-3">
      <div className="flex items-center justify-between gap-3">
        <div className="font-medium">{label}</div>
        <span className={`rounded-full px-2 py-0.5 text-xs ${tone}`}>
          {status.exists ? 'exists' : 'missing'}
          {status.writable ? ' · writable' : ' · read-only'}
        </span>
      </div>
      <div className="mt-2 truncate text-xs text-gray-500">{status.path}</div>
    </div>
  );
}

function FullScreenMessage({ title, body }: { title: string; body: string }) {
  return (
    <div className="flex min-h-screen items-center justify-center bg-background px-4">
      <div className="max-w-md text-center">
        <h1 className="text-3xl font-semibold">{title}</h1>
        <p className="mt-3 text-gray-400">{body}</p>
      </div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="block space-y-2">
      <span className="text-sm font-medium text-gray-300">{label}</span>
      {children}
    </label>
  );
}

function ErrorBanner({ error, onDismiss }: { error: string | null; onDismiss: () => void }) {
  if (!error) {
    return null;
  }

  return (
    <div className="mt-4 rounded-lg border border-error/30 bg-error/10 px-4 py-3 text-sm text-error">
      <div className="flex items-start justify-between gap-4">
        <p className="whitespace-pre-wrap">{error}</p>
        <button onClick={onDismiss} className="text-white/70 hover:text-white">
          ×
        </button>
      </div>
    </div>
  );
}

function SummaryCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl border border-surface2 bg-surface p-4">
      <p className="text-xs uppercase tracking-[0.2em] text-gray-500">{label}</p>
      <p className="mt-3 text-lg font-medium">{value}</p>
    </div>
  );
}

function DashboardPanel({
  title,
  subtitle,
  actionLabel,
  onAction,
  children,
}: {
  title: string;
  subtitle: string;
  actionLabel?: string;
  onAction?: () => void;
  children: React.ReactNode;
}) {
  return (
    <section className="rounded-xl border border-surface2 bg-surface p-4">
      <div className="mb-3 flex items-center justify-between gap-3">
        <div>
          <h2 className="text-lg font-semibold">{title}</h2>
          <p className="text-sm text-gray-400">{subtitle}</p>
        </div>
        {actionLabel && onAction ? (
          <button
            onClick={onAction}
            className="rounded-lg border border-surface2 px-3 py-2 text-sm hover:border-primary"
          >
            {actionLabel}
          </button>
        ) : null}
      </div>
      {children}
    </section>
  );
}

function EmptyState({ text }: { text: string }) {
  return <div className="rounded-lg border border-dashed border-surface p-4 text-sm text-gray-500">{text}</div>;
}

function StatusPill({
  tone,
  children,
}: {
  tone: 'default' | 'success' | 'warning' | 'error';
  children: React.ReactNode;
}) {
  const styles = {
    default: 'border-surface2 text-gray-300',
    success: 'border-success/30 bg-success/10 text-success',
    warning: 'border-warning/30 bg-warning/10 text-warning',
    error: 'border-error/30 bg-error/10 text-error',
  }[tone];

  return <div className={`rounded-full border px-3 py-1 text-sm ${styles}`}>{children}</div>;
}

function formatSeconds(value: number | null | undefined): string {
  if (!value || value <= 0) {
    return 'expired';
  }
  if (value < 60) {
    return `${value}s`;
  }
  const minutes = Math.floor(value / 60);
  const seconds = value % 60;
  return `${minutes}m ${seconds}s`;
}

function daemonClient() {
  return new RoveDaemonClient(readStoredToken() ?? undefined);
}

function normalizeEpochMillis(value: number) {
  return value < 1_000_000_000_000 ? value * 1000 : value;
}
