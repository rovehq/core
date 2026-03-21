'use client';

import { useEffect, useState } from 'react';
import Nav from '@/components/Nav';
import { useRoveStore } from '@/stores/roveStore';

export default function MessagesPage() {
  const {
    appState,
    authStatus,
    clearError,
    daemonUrl,
    error,
    hello,
    initialize,
    lock,
    login,
    reauth,
    refreshTasks,
    setupPassword,
    submitTask,
    tasks,
    ws,
  } = useRoveStore();
  const [input, setInput] = useState('');
  const [password, setPassword] = useState('');
  const [nodeName, setNodeName] = useState('my-device');
  const [mode, setMode] = useState('local_only');
  const [isSubmitting, setIsSubmitting] = useState(false);

  useEffect(() => {
    void initialize();
  }, [initialize]);

  const handleSubmit = async (e?: React.FormEvent) => {
    e?.preventDefault();
    if (!input.trim() || isSubmitting) return;

    setIsSubmitting(true);
    if (await submitTask(input.trim())) {
      setInput('');
    }
    setIsSubmitting(false);
  };

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

  if (appState === 'checking') {
    return <FullScreenMessage title="Connecting to Rove" body="Probing the local daemon and restoring your session." />;
  }

  if (appState === 'offline') {
    return (
      <AuthShell title="Local Daemon Not Reachable" subtitle="Start the daemon from the Rove menu bar app, then refresh.">
        <div className="space-y-4">
          <p className="text-sm text-gray-400 whitespace-pre-wrap">{error ?? 'The browser could not reach 127.0.0.1:47630.'}</p>
          <button
            onClick={() => void initialize()}
            className="w-full rounded-lg bg-primary px-4 py-3 font-medium hover:bg-primary/80"
          >
            Retry Connection
          </button>
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
          <SummaryCard label="Services" value={hello?.capabilities.services.join(', ') || 'none'} />
          <SummaryCard label="Extensions" value={String(hello?.capabilities.extensions.length ?? 0)} />
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

        <section className="overflow-hidden rounded-xl border border-surface2 bg-surface">
          <div className="border-b border-surface2 p-4">
            <h2 className="text-lg font-semibold">Recent Tasks</h2>
          </div>

          <div className="max-h-[540px] space-y-4 overflow-y-auto p-4">
            {tasks.length === 0 ? (
              <div className="rounded-xl border border-dashed border-surface2 p-8 text-center text-gray-500">
                No tasks yet. Start from the form above after unlocking the daemon.
              </div>
            ) : (
              tasks.map((task) => (
                <TaskCard
                  key={task.id}
                  task={task}
                  getStatusColor={getStatusColor}
                  formatDuration={formatDuration}
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

function TaskCard({ task, getStatusColor, formatDuration }: {
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
          {new Date(task.createdAt).toLocaleTimeString()}
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
