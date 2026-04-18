'use client';

import { useEffect, useState } from 'react';

import Nav from '@/components/Nav';
import {
  DaemonError,
  HookStatus,
  HookSummary,
  RoveDaemonClient,
  readStoredToken,
} from '@/lib/daemon';

export default function HooksPage() {
  const [status, setStatus] = useState<HookStatus | null>(null);
  const [selectedName, setSelectedName] = useState<string>('');
  const [selectedHook, setSelectedHook] = useState<HookSummary | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void refresh();
  }, []);

  async function refresh() {
    setLoading(true);
    setError(null);
    try {
      const nextStatus = await daemonClient().getHookStatus();
      setStatus(nextStatus);
      const nextName = selectedName || nextStatus.hooks[0]?.name || '';
      setSelectedName(nextName);
      if (nextName) {
        setSelectedHook(await daemonClient().inspectHook(nextName));
      } else {
        setSelectedHook(null);
      }
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setLoading(false);
    }
  }

  async function selectHook(name: string) {
    setSelectedName(name);
    setRefreshing(true);
    setError(null);
    try {
      setSelectedHook(await daemonClient().inspectHook(name));
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setRefreshing(false);
    }
  }

  return (
    <div className="min-h-screen flex flex-col">
      <header className="sticky top-0 z-10 bg-background/80 backdrop-blur border-b border-surface2">
        <div className="max-w-6xl mx-auto px-4 py-4 space-y-4">
          <div>
            <h1 className="text-2xl font-semibold">Lifecycle Hooks</h1>
            <p className="text-sm text-gray-400">
              Inspect discovered `BeforeToolCall` and `AfterToolCall` hooks on this node.
            </p>
          </div>
          <Nav />
        </div>
      </header>

      <main className="flex-1 max-w-6xl w-full mx-auto px-4 py-6 space-y-6">
        <section className="grid gap-4 md:grid-cols-3">
          <StatCard label="Discovered" value={`${status?.hooks.length ?? 0}`} />
          <StatCard
            label="Disabled"
            value={`${status?.hooks.filter((hook) => hook.disabled).length ?? 0}`}
          />
          <StatCard
            label="Sources"
            value={
              status?.hooks.length
                ? `${new Set(status.hooks.map((hook) => sourceLabel(hook.source_path))).size}`
                : '0'
            }
          />
        </section>

        {error ? (
          <div className="rounded-2xl border border-rose-500/40 bg-rose-500/10 p-4 text-sm text-rose-200">
            {error}
          </div>
        ) : null}

        <section className="grid gap-6 lg:grid-cols-[1.2fr_0.8fr]">
          <div className="rounded-3xl border border-surface2 bg-surface/70 p-5 shadow-card">
            <div className="mb-4 flex items-center justify-between gap-3">
              <div>
                <h2 className="text-lg font-semibold">Discovered Hooks</h2>
                <p className="text-sm text-gray-400">
                  Runtime discovery from workspace and config hook roots.
                </p>
              </div>
              <button
                type="button"
                onClick={() => void refresh()}
                disabled={loading || refreshing}
                className="rounded-xl border border-surface2 px-4 py-2 text-sm text-gray-200 hover:border-primary hover:text-white disabled:opacity-50"
              >
                Refresh
              </button>
            </div>

            {loading ? (
              <p className="text-sm text-gray-400">Loading hooks…</p>
            ) : status?.hooks.length ? (
              <div className="space-y-3">
                {status.hooks.map((hook) => {
                  const active = hook.name === selectedName;
                  return (
                    <button
                      key={hook.name}
                      type="button"
                      onClick={() => void selectHook(hook.name)}
                      className={`w-full rounded-2xl border p-4 text-left transition ${
                        active
                          ? 'border-primary/70 bg-primary/10'
                          : 'border-surface2 bg-background/40 hover:border-surface2/80 hover:bg-surface2/30'
                      }`}
                    >
                      <div className="flex flex-wrap items-start justify-between gap-3">
                        <div>
                          <div className="flex items-center gap-2">
                            <span className="font-medium text-white">{hook.name}</span>
                            {hook.disabled ? (
                              <span className="rounded-full border border-amber-500/40 bg-amber-500/10 px-2 py-0.5 text-[11px] uppercase tracking-wide text-amber-200">
                                Disabled
                              </span>
                            ) : (
                              <span className="rounded-full border border-emerald-500/40 bg-emerald-500/10 px-2 py-0.5 text-[11px] uppercase tracking-wide text-emerald-200">
                                Active
                              </span>
                            )}
                          </div>
                          <p className="mt-1 text-sm text-gray-300">
                            {hook.description ?? 'No description provided.'}
                          </p>
                        </div>
                        <div className="text-right text-xs text-gray-400">
                          <div>{hook.timeout_secs}s timeout</div>
                          <div>{hook.consecutive_failures} recent failures</div>
                        </div>
                      </div>
                      <div className="mt-3 flex flex-wrap gap-2">
                        {hook.events.map((event) => (
                          <span
                            key={event}
                            className="rounded-full border border-surface2 bg-background/60 px-2 py-1 text-[11px] text-gray-300"
                          >
                            {event}
                          </span>
                        ))}
                      </div>
                    </button>
                  );
                })}
              </div>
            ) : (
              <p className="text-sm text-gray-400">No lifecycle hooks discovered.</p>
            )}
          </div>

          <div className="rounded-3xl border border-surface2 bg-surface/70 p-5 shadow-card">
            <h2 className="text-lg font-semibold">Hook Detail</h2>
            <p className="mb-4 text-sm text-gray-400">
              Current runtime view of the selected hook definition.
            </p>

            {selectedHook ? (
              <div className="space-y-4 text-sm text-gray-200">
                <DetailRow label="Name" value={selectedHook.name} />
                <DetailRow
                  label="Description"
                  value={selectedHook.description ?? 'No description provided.'}
                />
                <DetailRow label="Command" value={selectedHook.command} mono />
                <DetailRow label="Source" value={selectedHook.source_path} mono />
                <DetailRow label="Events" value={selectedHook.events.join(', ')} />
                <DetailRow label="Timeout" value={`${selectedHook.timeout_secs}s`} />
                <DetailRow
                  label="Disabled"
                  value={selectedHook.disabled ? 'yes' : 'no'}
                />
                <DetailRow
                  label="Consecutive Failures"
                  value={`${selectedHook.consecutive_failures}`}
                />
                <DetailRow
                  label="Requires OS"
                  value={selectedHook.requires.os.join(', ') || 'none'}
                />
                <DetailRow
                  label="Requires Bins"
                  value={selectedHook.requires.bins.join(', ') || 'none'}
                  mono={selectedHook.requires.bins.length > 0}
                />
                <DetailRow
                  label="Requires Env"
                  value={selectedHook.requires.env.join(', ') || 'none'}
                  mono={selectedHook.requires.env.length > 0}
                />
              </div>
            ) : (
              <p className="text-sm text-gray-400">
                {loading ? 'Loading hook detail…' : 'Select a discovered hook to inspect it.'}
              </p>
            )}
          </div>
        </section>
      </main>
    </div>
  );
}

function StatCard({
  label,
  value,
}: {
  label: string;
  value: string;
}) {
  return (
    <div className="rounded-2xl border border-surface2 bg-surface/60 p-4 shadow-card">
      <div className="text-xs uppercase tracking-[0.2em] text-gray-500">{label}</div>
      <div className="mt-2 text-2xl font-semibold text-white">{value}</div>
    </div>
  );
}

function DetailRow({
  label,
  value,
  mono = false,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div className="rounded-2xl border border-surface2 bg-background/40 p-3">
      <div className="text-[11px] uppercase tracking-[0.16em] text-gray-500">{label}</div>
      <div className={`mt-1 break-all text-gray-100 ${mono ? 'font-mono text-[13px]' : ''}`}>
        {value}
      </div>
    </div>
  );
}

function daemonClient() {
  return new RoveDaemonClient(readStoredToken() ?? undefined);
}

function formatError(error: unknown) {
  if (error instanceof DaemonError) {
    return error.message;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return 'Unknown daemon error';
}

function sourceLabel(path: string) {
  if (path.includes('/.rove/hooks/')) {
    return 'workspace';
  }
  return 'config';
}
