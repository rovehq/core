'use client';

import { type ReactNode, useEffect, useState } from 'react';

import Nav from '@/components/Nav';
import {
  AgentSpec,
  DaemonError,
  ExecuteTaskResponse,
  RoveDaemonClient,
  AgentRunRecord,
  readStoredToken,
} from '@/lib/daemon';

const EMPTY_AGENT: AgentSpec = {
  schema_version: 1,
  id: '',
  name: '',
  purpose: 'Reusable Rove agent',
  instructions:
    'Help the user complete the assigned task safely and directly while respecting configured capabilities.',
  enabled: true,
  capabilities: [],
  channels: [],
  memory_policy: 'default',
  node_placement: {
    preferred_nodes: [],
    required_tags: [],
    allow_local: true,
    require_executor: false,
  },
  schedules: [],
  ui: {},
  tags: [],
};

export default function AgentsPage() {
  const [agents, setAgents] = useState<AgentSpec[]>([]);
  const [runs, setRuns] = useState<AgentRunRecord[]>([]);
  const [form, setForm] = useState<AgentSpec>(EMPTY_AGENT);
  const [runInput, setRunInput] = useState<Record<string, string>>({});
  const [runResult, setRunResult] = useState<Record<string, ExecuteTaskResponse>>({});
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void refresh();
  }, []);

  async function refresh() {
    setLoading(true);
    setError(null);
    try {
      const client = daemonClient();
      const [nextAgents, nextRuns] = await Promise.all([
        client.listAgents(),
        client.listAgentRuns(),
      ]);
      setAgents(nextAgents);
      setRuns(nextRuns);
      setForm((current) => {
        if (current.id) {
          const updated = nextAgents.find((item) => item.id === current.id);
          return updated ? cloneAgent(updated) : current;
        }
        return current;
      });
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setLoading(false);
    }
  }

  async function saveAgent() {
    setSaving(true);
    setError(null);
    try {
      const saved = await daemonClient().saveAgent(normalizeAgent(form));
      setForm(cloneAgent(saved));
      await refresh();
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setSaving(false);
    }
  }

  async function removeAgent(id: string) {
    if (typeof window !== 'undefined' && !window.confirm(`Remove agent '${id}'?`)) {
      return;
    }

    setError(null);
    try {
      await daemonClient().removeAgent(id);
      if (form.id === id) {
        setForm(cloneAgent(EMPTY_AGENT));
      }
      await refresh();
    } catch (nextError) {
      setError(formatError(nextError));
    }
  }

  async function runAgent(id: string) {
    const input = (runInput[id] ?? '').trim();
    if (!input) {
      setError(`Agent '${id}' requires a prompt to run.`);
      return;
    }

    setError(null);
    try {
      const result = await daemonClient().runAgent(id, input);
      setRunResult((current) => ({ ...current, [id]: result }));
      await refresh();
    } catch (nextError) {
      setError(formatError(nextError));
    }
  }

  return (
    <div className="min-h-screen flex flex-col">
      <header className="sticky top-0 z-10 bg-background/80 backdrop-blur border-b border-surface2">
        <div className="max-w-6xl mx-auto px-4 py-4 space-y-4">
          <div>
            <h1 className="text-2xl font-semibold">Agents</h1>
            <p className="text-sm text-gray-400">
              Manage reusable agent specs backed by the daemon runtime. This is the structured control plane that replaces prompt-file one-offs.
            </p>
          </div>
          <Nav />
        </div>
      </header>

      <main className="flex-1 max-w-6xl w-full mx-auto px-4 py-6 space-y-6">
        <section className="bg-surface rounded-xl p-6 border border-surface2 space-y-5">
          <div className="flex items-center justify-between gap-3">
            <div>
              <h2 className="text-lg font-semibold">Agent Spec</h2>
              <p className="text-sm text-gray-400">
                Create or edit a first-class agent. The daemon stores the spec as versioned TOML and applies it as a task execution profile at runtime.
              </p>
            </div>
            <div className="flex items-center gap-2">
              <button
                onClick={() => setForm(cloneAgent(EMPTY_AGENT))}
                className="rounded-lg border border-surface px-4 py-2 text-sm hover:border-primary"
              >
                New
              </button>
              <button
                onClick={() => void refresh()}
                className="rounded-lg border border-surface px-4 py-2 text-sm hover:border-primary"
              >
                Refresh
              </button>
            </div>
          </div>

          <div className="grid gap-4 md:grid-cols-2">
            <Field label="ID">
              <input
                value={form.id}
                onChange={(event) => setForm((current) => ({ ...current, id: event.target.value }))}
                className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                placeholder="default-assistant"
              />
            </Field>
            <Field label="Name">
              <input
                value={form.name}
                onChange={(event) => setForm((current) => ({ ...current, name: event.target.value }))}
                className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                placeholder="Default Assistant"
              />
            </Field>
            <Field label="Purpose">
              <input
                value={form.purpose}
                onChange={(event) => setForm((current) => ({ ...current, purpose: event.target.value }))}
                className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                placeholder="General-purpose local assistant"
              />
            </Field>
            <Field label="Memory Policy">
              <input
                value={form.memory_policy}
                onChange={(event) => setForm((current) => ({ ...current, memory_policy: event.target.value }))}
                className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                placeholder="default"
              />
            </Field>
            <Field label="Runtime Profile">
              <select
                value={form.runtime_profile ?? ''}
                onChange={(event) =>
                  setForm((current) => ({
                    ...current,
                    runtime_profile: event.target.value || null,
                  }))
                }
                className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
              >
                <option value="">Inherited</option>
                <option value="desktop">desktop</option>
                <option value="headless">headless</option>
              </select>
            </Field>
            <Field label="Approval Mode">
              <select
                value={form.approval_mode ?? ''}
                onChange={(event) =>
                  setForm((current) => ({
                    ...current,
                    approval_mode: event.target.value || null,
                  }))
                }
                className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
              >
                <option value="">Inherited</option>
                <option value="default">default</option>
                <option value="allowlist">allowlist</option>
                <option value="open">open</option>
                <option value="assisted">assisted</option>
              </select>
            </Field>
          </div>

          <Field label="Instructions">
            <textarea
              value={form.instructions}
              onChange={(event) => setForm((current) => ({ ...current, instructions: event.target.value }))}
              className="min-h-40 w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
              placeholder="System instructions for this agent"
            />
          </Field>

          <Field label="Output Contract">
            <textarea
              value={form.output_contract ?? ''}
              onChange={(event) =>
                setForm((current) => ({
                  ...current,
                  output_contract: emptyToNull(event.target.value),
                }))
              }
              className="min-h-24 w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
              placeholder="Optional structured output contract"
            />
          </Field>

          <div className="grid gap-4 md:grid-cols-2">
            <Field label="Tags">
              <input
                value={formatCsv(form.tags)}
                onChange={(event) => setForm((current) => ({ ...current, tags: parseCsv(event.target.value) }))}
                className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                placeholder="default, assistant"
              />
            </Field>
            <Field label="Preferred Nodes">
              <input
                value={formatCsv(form.node_placement.preferred_nodes)}
                onChange={(event) =>
                  setForm((current) => ({
                    ...current,
                    node_placement: {
                      ...current.node_placement,
                      preferred_nodes: parseCsv(event.target.value),
                    },
                  }))
                }
                className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                placeholder="home-mac, office-linux"
              />
            </Field>
            <Field label="Required Tags">
              <input
                value={formatCsv(form.node_placement.required_tags)}
                onChange={(event) =>
                  setForm((current) => ({
                    ...current,
                    node_placement: {
                      ...current.node_placement,
                      required_tags: parseCsv(event.target.value),
                    },
                  }))
                }
                className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                placeholder="gpu, trusted"
              />
            </Field>
            <div className="grid gap-3 sm:grid-cols-2">
              <Checkbox
                label="Enabled"
                checked={form.enabled}
                onChange={(checked) => setForm((current) => ({ ...current, enabled: checked }))}
              />
              <Checkbox
                label="Allow Local"
                checked={form.node_placement.allow_local}
                onChange={(checked) =>
                  setForm((current) => ({
                    ...current,
                    node_placement: { ...current.node_placement, allow_local: checked },
                  }))
                }
              />
              <Checkbox
                label="Require Executor"
                checked={form.node_placement.require_executor}
                onChange={(checked) =>
                  setForm((current) => ({
                    ...current,
                    node_placement: { ...current.node_placement, require_executor: checked },
                  }))
                }
              />
            </div>
          </div>

          <section className="space-y-3">
            <div className="flex items-center justify-between">
              <div>
                <h3 className="font-medium">Capabilities</h3>
                <p className="text-sm text-gray-400">
                  Capabilities are explicit bindings. Tool entries drive the runtime allow-list enforced during execution.
                </p>
              </div>
              <button
                onClick={() =>
                  setForm((current) => ({
                    ...current,
                    capabilities: [...current.capabilities, { kind: 'tool', name: '', required: false }],
                  }))
                }
                className="rounded-lg border border-surface px-3 py-2 text-sm hover:border-primary"
              >
                Add Capability
              </button>
            </div>
            {form.capabilities.length === 0 ? (
              <p className="rounded-lg bg-surface2 px-4 py-3 text-sm text-gray-400">
                No capabilities configured. Add at least one tool capability for bounded agents.
              </p>
            ) : (
              <div className="space-y-3">
                {form.capabilities.map((capability, index) => (
                  <div key={`${capability.kind}-${index}`} className="grid gap-3 rounded-lg bg-surface2 p-4 md:grid-cols-[1fr_2fr_auto_auto]">
                    <input
                      value={capability.kind}
                      onChange={(event) =>
                        setForm((current) => ({
                          ...current,
                          capabilities: current.capabilities.map((item, itemIndex) =>
                            itemIndex === index ? { ...item, kind: event.target.value } : item,
                          ),
                        }))
                      }
                      className="rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                      placeholder="tool"
                    />
                    <input
                      value={capability.name}
                      onChange={(event) =>
                        setForm((current) => ({
                          ...current,
                          capabilities: current.capabilities.map((item, itemIndex) =>
                            itemIndex === index ? { ...item, name: event.target.value } : item,
                          ),
                        }))
                      }
                      className="rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                      placeholder="write_file"
                    />
                    <Checkbox
                      label="Required"
                      checked={capability.required}
                      onChange={(checked) =>
                        setForm((current) => ({
                          ...current,
                          capabilities: current.capabilities.map((item, itemIndex) =>
                            itemIndex === index ? { ...item, required: checked } : item,
                          ),
                        }))
                      }
                    />
                    <button
                      onClick={() =>
                        setForm((current) => ({
                          ...current,
                          capabilities: current.capabilities.filter((_, itemIndex) => itemIndex !== index),
                        }))
                      }
                      className="rounded-lg border border-error/30 px-3 py-2 text-sm text-error hover:bg-error/10"
                    >
                      Remove
                    </button>
                  </div>
                ))}
              </div>
            )}
          </section>

          <div className="flex flex-wrap items-center gap-2">
            <button
              onClick={() => void saveAgent()}
              disabled={saving}
              className="rounded-lg bg-primary px-4 py-2 text-sm font-medium hover:bg-primary/80 disabled:cursor-not-allowed disabled:opacity-60"
            >
              {saving ? 'Saving…' : 'Save Agent'}
            </button>
            {form.id ? (
              <button
                onClick={() => void removeAgent(form.id)}
                className="rounded-lg border border-error/30 px-4 py-2 text-sm text-error hover:bg-error/10"
              >
                Remove
              </button>
            ) : null}
          </div>

          <ErrorBanner error={error} onDismiss={() => setError(null)} />
        </section>

        <section className="bg-surface rounded-xl p-6 border border-surface2 space-y-4">
          <div className="flex items-center justify-between">
            <div>
              <h2 className="text-lg font-semibold">Configured Agents</h2>
              <p className="text-sm text-gray-400">
                {loading ? 'Loading agents…' : `${agents.length} agent spec${agents.length === 1 ? '' : 's'} available`}
              </p>
            </div>
          </div>
          {agents.length === 0 ? (
            <EmptyState text="No agents are stored yet." />
          ) : (
            <div className="space-y-4">
              {agents.map((agent) => {
                const capabilityNames = agent.capabilities.map((capability) => `${capability.kind}:${capability.name}`);
                const result = runResult[agent.id];
                return (
                  <div key={agent.id} className="rounded-lg bg-surface2 p-4 space-y-4">
                    <div className="flex items-start justify-between gap-4">
                      <div>
                        <p className="font-medium">{agent.name}</p>
                        <p className="text-sm text-gray-500">
                          {agent.id} · {agent.enabled ? 'enabled' : 'disabled'} · {agent.runtime_profile ?? 'inherited profile'}
                        </p>
                        <p className="mt-2 text-sm text-gray-300">{agent.purpose}</p>
                        <p className="mt-2 text-sm text-gray-500">
                          capabilities {capabilityNames.join(', ') || 'none'}
                        </p>
                      </div>
                      <button
                        onClick={() => setForm(cloneAgent(agent))}
                        className="rounded-lg border border-surface px-3 py-2 text-sm hover:border-primary"
                      >
                        Edit
                      </button>
                    </div>
                    <div className="grid gap-3 md:grid-cols-[1fr_auto]">
                      <input
                        value={runInput[agent.id] ?? ''}
                        onChange={(event) =>
                          setRunInput((current) => ({ ...current, [agent.id]: event.target.value }))
                        }
                        className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                        placeholder={`Run ${agent.name} with a one-off input`}
                      />
                      <button
                        onClick={() => void runAgent(agent.id)}
                        className="rounded-lg bg-primary px-4 py-2 text-sm font-medium hover:bg-primary/80"
                      >
                        Run Once
                      </button>
                    </div>
                    {result ? (
                      <div className="rounded-lg border border-surface px-4 py-3 text-sm text-gray-300 space-y-1">
                        <p>Status: {result.status}</p>
                        {result.task_id ? <p>Task: <span className="font-mono">{result.task_id}</span></p> : null}
                        {result.message ? <p>Run: <span className="font-mono">{result.message}</span></p> : null}
                        {result.answer ? <p className="whitespace-pre-wrap">{result.answer}</p> : null}
                      </div>
                    ) : null}
                  </div>
                );
              })}
            </div>
          )}
        </section>

        <section className="bg-surface rounded-xl p-6 border border-surface2 space-y-4">
          <div>
            <h2 className="text-lg font-semibold">Recent Agent Runs</h2>
            <p className="text-sm text-gray-400">Run records are persisted in SQLite and survive daemon restarts.</p>
          </div>
          {runs.length === 0 ? (
            <EmptyState text="No agent runs recorded yet." />
          ) : (
            <div className="space-y-3">
              {runs.map((run) => (
                <div key={run.run_id} className="rounded-lg bg-surface2 p-4">
                  <p className="font-medium">{run.agent_id}</p>
                  <p className="text-sm text-gray-500">
                    {run.status} · {formatTimestamp(run.created_at)}{run.completed_at ? ` → ${formatTimestamp(run.completed_at)}` : ''}
                  </p>
                  <p className="mt-2 text-sm text-gray-300 whitespace-pre-wrap">{run.input}</p>
                  {run.output ? <p className="mt-2 text-sm text-gray-400 whitespace-pre-wrap">{run.output}</p> : null}
                  {run.error ? <p className="mt-2 text-sm text-error whitespace-pre-wrap">{run.error}</p> : null}
                </div>
              ))}
            </div>
          )}
        </section>
      </main>
    </div>
  );
}

function daemonClient() {
  return new RoveDaemonClient(readStoredToken() ?? undefined);
}

function cloneAgent(spec: AgentSpec): AgentSpec {
  return {
    ...spec,
    capabilities: spec.capabilities.map((capability) => ({ ...capability })),
    channels: spec.channels.map((channel) => ({ ...channel })),
    node_placement: {
      preferred_nodes: [...spec.node_placement.preferred_nodes],
      required_tags: [...spec.node_placement.required_tags],
      allow_local: spec.node_placement.allow_local,
      require_executor: spec.node_placement.require_executor,
    },
    schedules: [...spec.schedules],
    tags: [...spec.tags],
    ui: { ...spec.ui },
  };
}

function normalizeAgent(spec: AgentSpec): AgentSpec {
  return {
    ...spec,
    id: spec.id.trim(),
    name: spec.name.trim(),
    purpose: spec.purpose.trim(),
    instructions: spec.instructions.trim(),
    model_policy: emptyToNull(spec.model_policy),
    memory_policy: spec.memory_policy.trim() || 'default',
    approval_mode: emptyToNull(spec.approval_mode),
    runtime_profile: emptyToNull(spec.runtime_profile),
    output_contract: emptyToNull(spec.output_contract),
    tags: spec.tags.map((tag) => tag.trim()).filter(Boolean),
    capabilities: spec.capabilities
      .map((capability) => ({
        ...capability,
        kind: capability.kind.trim(),
        name: capability.name.trim(),
      }))
      .filter((capability) => capability.kind && capability.name),
    node_placement: {
      ...spec.node_placement,
      preferred_nodes: spec.node_placement.preferred_nodes.map((value) => value.trim()).filter(Boolean),
      required_tags: spec.node_placement.required_tags.map((value) => value.trim()).filter(Boolean),
    },
  };
}

function formatCsv(values: string[]) {
  return values.join(', ');
}

function parseCsv(value: string) {
  return value
    .split(',')
    .map((item) => item.trim())
    .filter(Boolean);
}

function emptyToNull(value?: string | null) {
  const normalized = value?.trim() ?? '';
  return normalized ? normalized : null;
}

function formatTimestamp(timestamp: number) {
  return new Date(timestamp * 1000).toLocaleString();
}

function formatError(error: unknown) {
  if (error instanceof DaemonError) {
    return error.message;
  }
  return error instanceof Error ? error.message : 'Unknown daemon error';
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="space-y-2">
      <span className="text-sm text-gray-400">{label}</span>
      {children}
    </label>
  );
}

function Checkbox({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="flex items-center gap-2 rounded-lg border border-surface bg-background px-3 py-2 text-sm">
      <input type="checkbox" checked={checked} onChange={(event) => onChange(event.target.checked)} />
      {label}
    </label>
  );
}

function EmptyState({ text }: { text: string }) {
  return <div className="rounded-lg bg-surface2 px-4 py-3 text-sm text-gray-400">{text}</div>;
}

function ErrorBanner({ error, onDismiss }: { error: string | null; onDismiss: () => void }) {
  if (!error) return null;
  return (
    <div className="rounded-lg border border-error/30 bg-error/10 px-4 py-3 text-sm text-error">
      <div className="flex items-start justify-between gap-3">
        <p>{error}</p>
        <button onClick={onDismiss}>×</button>
      </div>
    </div>
  );
}
