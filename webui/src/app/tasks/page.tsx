'use client';

import type { ReactNode } from 'react';
import { useEffect, useMemo, useState } from 'react';

import Nav from '@/components/Nav';
import {
  DaemonError,
  TaskAgentFacet,
  TaskEventsResponse,
  TaskStreamEvent,
  TaskSummary,
  RoveDaemonClient,
  readStoredToken,
} from '@/lib/daemon';

type TaskStatusFilter = 'all' | TaskSummary['status'];

const DEFAULT_LIMIT = 50;

export default function TasksPage() {
  const [tasks, setTasks] = useState<TaskSummary[]>([]);
  const [agents, setAgents] = useState<TaskAgentFacet[]>([]);
  const [selectedTaskId, setSelectedTaskId] = useState<string>('');
  const [selectedTask, setSelectedTask] = useState<TaskEventsResponse | null>(null);
  const [statusFilter, setStatusFilter] = useState<TaskStatusFilter>('all');
  const [agentFilter, setAgentFilter] = useState('all');
  const [threadFilter, setThreadFilter] = useState('');
  const [dateFrom, setDateFrom] = useState('');
  const [dateTo, setDateTo] = useState('');
  const [loading, setLoading] = useState(true);
  const [detailLoading, setDetailLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void refresh();
  }, []);

  async function refresh() {
    setLoading(true);
    setError(null);
    try {
      const [nextTasks, nextAgents] = await Promise.all([
        daemonClient().listTasks(buildFilters()),
        daemonClient().listTaskAgents(),
      ]);
      setTasks(nextTasks);
      setAgents(nextAgents);

      const preferredTaskId =
        nextTasks.find((task) => task.id === selectedTaskId)?.id ?? nextTasks[0]?.id ?? '';
      setSelectedTaskId(preferredTaskId);

      if (preferredTaskId) {
        setSelectedTask(await daemonClient().getTaskEvents(preferredTaskId));
      } else {
        setSelectedTask(null);
      }
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setLoading(false);
    }
  }

  async function selectTask(taskId: string) {
    setSelectedTaskId(taskId);
    setDetailLoading(true);
    setError(null);
    try {
      setSelectedTask(await daemonClient().getTaskEvents(taskId));
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setDetailLoading(false);
    }
  }

  function buildFilters() {
    return {
      status: statusFilter === 'all' ? undefined : statusFilter,
      agent_id: agentFilter === 'all' ? undefined : agentFilter,
      thread_id: threadFilter.trim() || undefined,
      date_from: dateFrom ? startOfDayUnix(dateFrom) : undefined,
      date_to: dateTo ? endOfDayUnix(dateTo) : undefined,
      limit: DEFAULT_LIMIT,
      offset: 0,
    };
  }

  const selectedFinalAnswer = useMemo(() => {
    const events = selectedTask?.stream_events ?? [];
    return [...events].reverse().find((event) => event.phase === 'final_answer')?.detail ?? null;
  }, [selectedTask]);

  const normalizedEvents = selectedTask?.stream_events ?? [];
  const rawEvents = selectedTask?.events ?? [];

  return (
    <div className="min-h-screen flex flex-col">
      <header className="sticky top-0 z-10 bg-background/80 backdrop-blur border-b border-surface2">
        <div className="max-w-7xl mx-auto px-4 py-4 space-y-4">
          <div>
            <h1 className="text-2xl font-semibold">Task History</h1>
            <p className="text-sm text-gray-400">
              Durable task history with agent, date, status, final output, and per-turn traces.
            </p>
          </div>
          <Nav />
        </div>
      </header>

      <main className="flex-1 max-w-7xl w-full mx-auto px-4 py-6 space-y-6">
        <section className="grid gap-4 md:grid-cols-2 xl:grid-cols-5">
          <StatCard label="Loaded Tasks" value={`${tasks.length}`} />
          <StatCard label="Completed" value={`${countStatus(tasks, 'completed')}`} />
          <StatCard label="Running" value={`${countStatus(tasks, 'running')}`} />
          <StatCard label="Failed" value={`${countStatus(tasks, 'failed')}`} />
          <StatCard label="Agents" value={`${agents.length}`} />
        </section>

        {error ? (
          <div className="rounded-2xl border border-rose-500/40 bg-rose-500/10 p-4 text-sm text-rose-200">
            {error}
          </div>
        ) : null}

        <section className="rounded-3xl border border-surface2 bg-surface/70 p-5 shadow-card space-y-4">
          <div className="flex flex-wrap items-start justify-between gap-4">
            <div>
              <h2 className="text-lg font-semibold">Filters</h2>
              <p className="text-sm text-gray-400">
                Query past tasks by date, agent, and run status.
              </p>
            </div>
            <button
              type="button"
              onClick={() => void refresh()}
              disabled={loading}
              className="rounded-xl border border-surface2 px-4 py-2 text-sm text-gray-200 hover:border-primary hover:text-white disabled:opacity-50"
            >
              Refresh
            </button>
          </div>

          <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-5">
            <FilterField label="Status">
              <select
                value={statusFilter}
                onChange={(event) => setStatusFilter(event.target.value as TaskStatusFilter)}
                className={inputClassName}
              >
                <option value="all">All statuses</option>
                <option value="pending">Pending</option>
                <option value="running">Running</option>
                <option value="completed">Completed</option>
                <option value="failed">Failed</option>
              </select>
            </FilterField>

            <FilterField label="Agent">
              <select
                value={agentFilter}
                onChange={(event) => setAgentFilter(event.target.value)}
                className={inputClassName}
              >
                <option value="all">All agents</option>
                {agents.map((agent) => (
                  <option key={agent.agent_id} value={agent.agent_id}>
                    {agent.agent_name || agent.agent_id}
                  </option>
                ))}
              </select>
            </FilterField>

            <FilterField label="Date From">
              <input
                type="date"
                value={dateFrom}
                onChange={(event) => setDateFrom(event.target.value)}
                className={inputClassName}
              />
            </FilterField>

            <FilterField label="Date To">
              <input
                type="date"
                value={dateTo}
                onChange={(event) => setDateTo(event.target.value)}
                className={inputClassName}
              />
            </FilterField>

            <FilterField label="Thread Id">
              <input
                type="text"
                value={threadFilter}
                onChange={(event) => setThreadFilter(event.target.value)}
                className={inputClassName}
                placeholder="workflow:release:thread:research"
              />
            </FilterField>

            <div className="flex items-end gap-3">
              <button
                type="button"
                onClick={() => void refresh()}
                disabled={loading}
                className="rounded-xl bg-primary px-4 py-2 text-sm font-medium text-white hover:bg-primary/90 disabled:opacity-50"
              >
                Apply
              </button>
              <button
                type="button"
                onClick={() => {
                  setStatusFilter('all');
                  setAgentFilter('all');
                  setThreadFilter('');
                  setDateFrom('');
                  setDateTo('');
                }}
                className="rounded-xl border border-surface2 px-4 py-2 text-sm text-gray-200 hover:border-primary hover:text-white"
              >
                Clear
              </button>
            </div>
          </div>
        </section>

        <section className="grid gap-6 xl:grid-cols-[1.05fr_1.35fr]">
          <div className="rounded-3xl border border-surface2 bg-surface/70 p-5 shadow-card">
            <div className="mb-4 flex items-center justify-between gap-3">
              <div>
                <h2 className="text-lg font-semibold">Task Runs</h2>
                <p className="text-sm text-gray-400">
                  Showing up to {DEFAULT_LIMIT} persisted tasks from the daemon store.
                </p>
              </div>
            </div>

            {loading ? (
              <p className="text-sm text-gray-400">Loading tasks…</p>
            ) : tasks.length ? (
              <div className="space-y-3">
                {tasks.map((task) => {
                  const active = task.id === selectedTaskId;
                  return (
                    <button
                      key={task.id}
                      type="button"
                      onClick={() => void selectTask(task.id)}
                      className={`w-full rounded-2xl border p-4 text-left transition ${
                        active
                          ? 'border-primary/70 bg-primary/10'
                          : 'border-surface2 bg-background/40 hover:border-surface2/80 hover:bg-surface2/30'
                      }`}
                    >
                      <div className="flex flex-wrap items-start justify-between gap-3">
                        <div className="min-w-0 flex-1">
                          <div className="flex flex-wrap items-center gap-2">
                            <StatusBadge status={task.status} />
                            <span className="rounded-full border border-surface2 bg-background/60 px-2 py-0.5 text-[11px] uppercase tracking-wide text-gray-300">
                              {task.agent_name || task.agent_id || 'unattributed'}
                            </span>
                            {task.thread_id ? (
                              <span className="rounded-full border border-surface2 bg-background/60 px-2 py-0.5 text-[11px] text-gray-300">
                                {task.thread_id}
                              </span>
                            ) : null}
                            {task.worker_preset_name ? (
                              <span className="rounded-full border border-surface2 bg-background/60 px-2 py-0.5 text-[11px] uppercase tracking-wide text-gray-300">
                                {task.worker_preset_name}
                              </span>
                            ) : null}
                            <span className="rounded-full border border-surface2 bg-background/60 px-2 py-0.5 text-[11px] uppercase tracking-wide text-gray-400">
                              {task.source}
                            </span>
                          </div>
                          <div className="mt-2 line-clamp-2 text-sm text-white">{task.input}</div>
                        </div>
                        <div className="text-right text-xs text-gray-400">
                          <div>{formatEpoch(task.created_at)}</div>
                          <div>{formatDuration(task.duration_ms)}</div>
                        </div>
                      </div>
                    </button>
                  );
                })}
              </div>
            ) : (
              <p className="text-sm text-gray-400">No tasks matched the current filters.</p>
            )}
          </div>

          <div className="space-y-6">
            <div className="rounded-3xl border border-surface2 bg-surface/70 p-5 shadow-card">
              <div className="mb-4">
                <h2 className="text-lg font-semibold">Task Detail</h2>
                <p className="text-sm text-gray-400">
                  Final answer, metadata, and normalized turn trace for the selected task.
                </p>
              </div>

              {detailLoading ? (
                <p className="text-sm text-gray-400">Loading task detail…</p>
              ) : selectedTask ? (
                <div className="space-y-4">
                  <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
                    <StatCard label="Status" value={selectedTask.task.status} />
                    <StatCard
                      label="Agent"
                      value={selectedTask.task.agent_name || selectedTask.task.agent_id || 'n/a'}
                    />
                    <StatCard
                      label="Thread"
                      value={selectedTask.task.thread_id || 'n/a'}
                      detail={selectedTask.task.worker_preset_name || undefined}
                    />
                    <StatCard label="Source" value={selectedTask.task.source} />
                    <StatCard
                      label="Trace Events"
                      value={`${selectedTask.stream_events.length}`}
                      detail={`${selectedTask.events.length} raw`}
                    />
                  </div>

                  <DetailBlock label="Input" mono={false}>
                    {selectedTask.task.input}
                  </DetailBlock>

                  <DetailBlock label="Final Output" mono={false}>
                    {selectedFinalAnswer || 'No final answer captured for this task.'}
                  </DetailBlock>

                  <div className="grid gap-4 md:grid-cols-2">
                    <DetailBlock label="Created" mono={false}>
                      {formatEpoch(selectedTask.task.created_at)}
                    </DetailBlock>
                    <DetailBlock label="Completed" mono={false}>
                      {selectedTask.task.completed_at
                        ? formatEpoch(selectedTask.task.completed_at)
                        : 'still running'}
                    </DetailBlock>
                    <DetailBlock label="Provider" mono={false}>
                      {selectedTask.task.provider_used || 'n/a'}
                    </DetailBlock>
                    <DetailBlock label="Worker Preset" mono={false}>
                      {selectedTask.task.worker_preset_name || selectedTask.task.worker_preset_id || 'n/a'}
                    </DetailBlock>
                    <DetailBlock label="Duration" mono={false}>
                      {formatDuration(selectedTask.task.duration_ms)}
                    </DetailBlock>
                  </div>
                </div>
              ) : (
                <p className="text-sm text-gray-400">Select a task to inspect its history.</p>
              )}
            </div>

            <div className="rounded-3xl border border-surface2 bg-surface/70 p-5 shadow-card">
              <h2 className="text-lg font-semibold">Normalized Turn Trace</h2>
              <p className="mb-4 text-sm text-gray-400">
                Canonical per-turn event stream used by the live runner and durable history.
              </p>

              {normalizedEvents.length ? (
                <div className="space-y-3">
                  {normalizedEvents.map((event) => (
                    <TraceRow key={event.id} event={event} />
                  ))}
                </div>
              ) : (
                <p className="text-sm text-gray-400">No normalized trace events recorded.</p>
              )}
            </div>

            <div className="rounded-3xl border border-surface2 bg-surface/70 p-5 shadow-card">
              <h2 className="text-lg font-semibold">Raw Stored Events</h2>
              <p className="mb-4 text-sm text-gray-400">
                Raw event store payloads for debugging serializer output and replay fidelity.
              </p>

              {rawEvents.length ? (
                <div className="space-y-3">
                  {rawEvents.map((event) => (
                    <div
                      key={event.id}
                      className="rounded-2xl border border-surface2 bg-background/40 p-4"
                    >
                      <div className="flex flex-wrap items-center justify-between gap-3">
                        <div className="font-medium text-white">{event.event_type}</div>
                        <div className="text-xs text-gray-400">
                          step {event.step_num} · {formatEpoch(event.created_at)}
                        </div>
                      </div>
                      <pre className="mt-3 overflow-x-auto whitespace-pre-wrap break-words rounded-xl border border-surface2 bg-background/70 p-3 text-xs text-gray-200">
                        {formatPayload(event.payload)}
                      </pre>
                    </div>
                  ))}
                </div>
              ) : (
                <p className="text-sm text-gray-400">No raw events stored for this task.</p>
              )}
            </div>
          </div>
        </section>
      </main>
    </div>
  );
}

function StatCard({
  label,
  value,
  detail,
}: {
  label: string;
  value: string;
  detail?: string;
}) {
  return (
    <div className="rounded-2xl border border-surface2 bg-surface/60 p-4 shadow-card">
      <div className="text-xs uppercase tracking-[0.2em] text-gray-500">{label}</div>
      <div className="mt-2 text-2xl font-semibold text-white break-words">{value}</div>
      {detail ? <div className="mt-1 text-xs text-gray-400">{detail}</div> : null}
    </div>
  );
}

function FilterField({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <label className="space-y-2 text-sm text-gray-300">
      <span className="block text-[11px] uppercase tracking-[0.16em] text-gray-500">{label}</span>
      {children}
    </label>
  );
}

function DetailBlock({
  label,
  children,
  mono = true,
}: {
  label: string;
  children: ReactNode;
  mono?: boolean;
}) {
  return (
    <div className="rounded-2xl border border-surface2 bg-background/40 p-4">
      <div className="text-[11px] uppercase tracking-[0.16em] text-gray-500">{label}</div>
      <div className={`mt-2 whitespace-pre-wrap break-words text-gray-100 ${mono ? 'font-mono text-[13px]' : 'text-sm'}`}>
        {children}
      </div>
    </div>
  );
}

function StatusBadge({ status }: { status: TaskSummary['status'] }) {
  const tone =
    status === 'completed'
      ? 'border-emerald-500/40 bg-emerald-500/10 text-emerald-200'
      : status === 'failed'
        ? 'border-rose-500/40 bg-rose-500/10 text-rose-200'
        : status === 'running'
          ? 'border-sky-500/40 bg-sky-500/10 text-sky-200'
          : 'border-amber-500/40 bg-amber-500/10 text-amber-200';
  return (
    <span className={`rounded-full border px-2 py-0.5 text-[11px] uppercase tracking-wide ${tone}`}>
      {status}
    </span>
  );
}

function TraceRow({ event }: { event: TaskStreamEvent }) {
  return (
    <div className="rounded-2xl border border-surface2 bg-background/40 p-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <span className="rounded-full border border-surface2 bg-background/60 px-2 py-0.5 text-[11px] uppercase tracking-wide text-gray-300">
              {event.phase}
            </span>
            <span className="text-sm font-medium text-white">{event.summary}</span>
            {event.tool_name ? (
              <span className="rounded-full border border-primary/30 bg-primary/10 px-2 py-0.5 text-[11px] uppercase tracking-wide text-primary">
                {event.tool_name}
              </span>
            ) : null}
          </div>
          {event.detail ? (
            <p className="mt-2 whitespace-pre-wrap break-words text-sm text-gray-300">
              {event.detail}
            </p>
          ) : null}
        </div>
        <div className="text-right text-xs text-gray-400">
          <div>step {event.step_num}</div>
          <div>{formatEpoch(event.created_at)}</div>
        </div>
      </div>
    </div>
  );
}

const inputClassName =
  'w-full rounded-xl border border-surface2 bg-background/60 px-3 py-2 text-sm text-white outline-none transition focus:border-primary';

function countStatus(tasks: TaskSummary[], status: TaskSummary['status']) {
  return tasks.filter((task) => task.status === status).length;
}

function formatEpoch(epochSeconds: number) {
  return new Date(epochSeconds * 1000).toLocaleString();
}

function formatDuration(durationMs?: number | null) {
  if (typeof durationMs !== 'number') {
    return 'n/a';
  }
  if (durationMs < 1000) {
    return `${durationMs} ms`;
  }
  const seconds = durationMs / 1000;
  if (seconds < 60) {
    return `${seconds.toFixed(1)} s`;
  }
  const minutes = Math.floor(seconds / 60);
  const remainderSeconds = Math.round(seconds % 60);
  return `${minutes}m ${remainderSeconds}s`;
}

function startOfDayUnix(value: string) {
  const date = new Date(`${value}T00:00:00`);
  return Math.floor(date.getTime() / 1000);
}

function endOfDayUnix(value: string) {
  const date = new Date(`${value}T23:59:59`);
  return Math.floor(date.getTime() / 1000);
}

function formatPayload(payload: string) {
  try {
    return JSON.stringify(JSON.parse(payload), null, 2);
  } catch {
    return payload;
  }
}

function daemonClient() {
  return new RoveDaemonClient(readStoredToken() ?? undefined);
}

function formatError(error: unknown) {
  if (error instanceof DaemonError) {
    return `${error.message} (${error.status})`;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}
