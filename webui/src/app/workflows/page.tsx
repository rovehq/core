'use client';

import { type ReactNode, useEffect, useState } from 'react';

import Nav from '@/components/Nav';
import {
  AgentSpec,
  DaemonError,
  ExecuteTaskResponse,
  RoveDaemonClient,
  WorkflowRunRecord,
  WorkflowSpec,
  WorkflowStepSpec,
  readStoredToken,
} from '@/lib/daemon';

const EMPTY_WORKFLOW: WorkflowSpec = {
  schema_version: 1,
  id: '',
  name: '',
  description: 'Reusable Rove workflow',
  enabled: true,
  steps: [
    {
      id: 'step-1',
      name: 'Step 1',
      prompt: '',
      continue_on_error: false,
    },
  ],
  tags: [],
};

export default function WorkflowsPage() {
  const [workflows, setWorkflows] = useState<WorkflowSpec[]>([]);
  const [agents, setAgents] = useState<AgentSpec[]>([]);
  const [runs, setRuns] = useState<WorkflowRunRecord[]>([]);
  const [form, setForm] = useState<WorkflowSpec>(EMPTY_WORKFLOW);
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
      const [nextWorkflows, nextAgents, nextRuns] = await Promise.all([
        client.listWorkflows(),
        client.listAgents(),
        client.listWorkflowRuns(),
      ]);
      setWorkflows(nextWorkflows);
      setAgents(nextAgents);
      setRuns(nextRuns);
      setForm((current) => {
        if (current.id) {
          const updated = nextWorkflows.find((item) => item.id === current.id);
          return updated ? cloneWorkflow(updated) : current;
        }
        return current;
      });
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setLoading(false);
    }
  }

  async function saveWorkflow() {
    setSaving(true);
    setError(null);
    try {
      const saved = await daemonClient().saveWorkflow(normalizeWorkflow(form));
      setForm(cloneWorkflow(saved));
      await refresh();
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setSaving(false);
    }
  }

  async function removeWorkflow(id: string) {
    if (typeof window !== 'undefined' && !window.confirm(`Remove workflow '${id}'?`)) {
      return;
    }

    setError(null);
    try {
      await daemonClient().removeWorkflow(id);
      if (form.id === id) {
        setForm(cloneWorkflow(EMPTY_WORKFLOW));
      }
      await refresh();
    } catch (nextError) {
      setError(formatError(nextError));
    }
  }

  async function runWorkflow(id: string) {
    const input = (runInput[id] ?? '').trim();
    if (!input) {
      setError(`Workflow '${id}' requires an input.`);
      return;
    }

    setError(null);
    try {
      const result = await daemonClient().runWorkflow(id, input);
      setRunResult((current) => ({ ...current, [id]: result }));
      await refresh();
    } catch (nextError) {
      setError(formatError(nextError));
    }
  }

  function updateStep(index: number, patch: Partial<WorkflowStepSpec>) {
    setForm((current) => ({
      ...current,
      steps: current.steps.map((step, stepIndex) =>
        stepIndex === index ? { ...step, ...patch } : step,
      ),
    }));
  }

  return (
    <div className="min-h-screen flex flex-col">
      <header className="sticky top-0 z-10 bg-background/80 backdrop-blur border-b border-surface2">
        <div className="max-w-6xl mx-auto px-4 py-4 space-y-4">
          <div>
            <h1 className="text-2xl font-semibold">Workflows</h1>
            <p className="text-sm text-gray-400">
              Compose multi-step runs that use shared daemon execution, policy, approvals, and optional agent profiles without creating a second workflow engine.
            </p>
          </div>
          <Nav />
        </div>
      </header>

      <main className="flex-1 max-w-6xl w-full mx-auto px-4 py-6 space-y-6">
        <section className="bg-surface rounded-xl p-6 border border-surface2 space-y-5">
          <div className="flex items-center justify-between gap-3">
            <div>
              <h2 className="text-lg font-semibold">Workflow Spec</h2>
              <p className="text-sm text-gray-400">
                Each step can run directly or inherit an agent profile. <code>{'{{input}}'}</code> and <code>{'{{last_output}}'}</code> are available in step prompts.
              </p>
            </div>
            <div className="flex items-center gap-2">
              <button
                onClick={() => setForm(cloneWorkflow(EMPTY_WORKFLOW))}
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
                placeholder="release-checklist"
              />
            </Field>
            <Field label="Name">
              <input
                value={form.name}
                onChange={(event) => setForm((current) => ({ ...current, name: event.target.value }))}
                className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                placeholder="Release Checklist"
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
            <Field label="Tags">
              <input
                value={formatCsv(form.tags)}
                onChange={(event) => setForm((current) => ({ ...current, tags: parseCsv(event.target.value) }))}
                className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                placeholder="deploy, release"
              />
            </Field>
          </div>

          <Field label="Description">
            <textarea
              value={form.description}
              onChange={(event) => setForm((current) => ({ ...current, description: event.target.value }))}
              className="min-h-24 w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
              placeholder="Describe what this workflow automates"
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
              placeholder="Optional final workflow output contract"
            />
          </Field>

          <Checkbox
            label="Enabled"
            checked={form.enabled}
            onChange={(checked) => setForm((current) => ({ ...current, enabled: checked }))}
          />

          <section className="space-y-3">
            <div className="flex items-center justify-between">
              <div>
                <h3 className="font-medium">Steps</h3>
                <p className="text-sm text-gray-400">
                  Steps execute in order. If an agent is selected, that step runs with the agent’s execution profile and tool allow-list.
                </p>
              </div>
              <button
                onClick={() =>
                  setForm((current) => ({
                    ...current,
                    steps: [
                      ...current.steps,
                      {
                        id: `step-${current.steps.length + 1}`,
                        name: `Step ${current.steps.length + 1}`,
                        prompt: '',
                        continue_on_error: false,
                      },
                    ],
                  }))
                }
                className="rounded-lg border border-surface px-3 py-2 text-sm hover:border-primary"
              >
                Add Step
              </button>
            </div>

            {form.steps.map((step, index) => (
              <div key={step.id || index} className="rounded-lg bg-surface2 p-4 space-y-3">
                <div className="grid gap-3 md:grid-cols-3">
                  <input
                    value={step.id}
                    onChange={(event) => updateStep(index, { id: event.target.value })}
                    className="rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                    placeholder="step-1"
                  />
                  <input
                    value={step.name}
                    onChange={(event) => updateStep(index, { name: event.target.value })}
                    className="rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                    placeholder="Step name"
                  />
                  <select
                    value={step.agent_id ?? ''}
                    onChange={(event) => updateStep(index, { agent_id: event.target.value || null })}
                    className="rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                  >
                    <option value="">No agent profile</option>
                    {agents.map((agent) => (
                      <option key={agent.id} value={agent.id}>
                        {agent.name} ({agent.id})
                      </option>
                    ))}
                  </select>
                </div>
                <textarea
                  value={step.prompt}
                  onChange={(event) => updateStep(index, { prompt: event.target.value })}
                  className="min-h-28 w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                  placeholder="Use {{input}} and {{last_output}} when needed"
                />
                <div className="flex items-center justify-between gap-3">
                  <Checkbox
                    label="Continue On Error"
                    checked={step.continue_on_error}
                    onChange={(checked) => updateStep(index, { continue_on_error: checked })}
                  />
                  <button
                    onClick={() =>
                      setForm((current) => ({
                        ...current,
                        steps: current.steps.filter((_, stepIndex) => stepIndex !== index),
                      }))
                    }
                    disabled={form.steps.length === 1}
                    className="rounded-lg border border-error/30 px-3 py-2 text-sm text-error hover:bg-error/10 disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    Remove Step
                  </button>
                </div>
              </div>
            ))}
          </section>

          <div className="flex flex-wrap items-center gap-2">
            <button
              onClick={() => void saveWorkflow()}
              disabled={saving}
              className="rounded-lg bg-primary px-4 py-2 text-sm font-medium hover:bg-primary/80 disabled:cursor-not-allowed disabled:opacity-60"
            >
              {saving ? 'Saving…' : 'Save Workflow'}
            </button>
            {form.id ? (
              <button
                onClick={() => void removeWorkflow(form.id)}
                className="rounded-lg border border-error/30 px-4 py-2 text-sm text-error hover:bg-error/10"
              >
                Remove
              </button>
            ) : null}
          </div>

          <ErrorBanner error={error} onDismiss={() => setError(null)} />
        </section>

        <section className="bg-surface rounded-xl p-6 border border-surface2 space-y-4">
          <div>
            <h2 className="text-lg font-semibold">Configured Workflows</h2>
            <p className="text-sm text-gray-400">
              {loading ? 'Loading workflows…' : `${workflows.length} workflow spec${workflows.length === 1 ? '' : 's'} available`}
            </p>
          </div>
          {workflows.length === 0 ? (
            <EmptyState text="No workflows are stored yet." />
          ) : (
            <div className="space-y-4">
              {workflows.map((workflow) => {
                const result = runResult[workflow.id];
                return (
                  <div key={workflow.id} className="rounded-lg bg-surface2 p-4 space-y-4">
                    <div className="flex items-start justify-between gap-4">
                      <div>
                        <p className="font-medium">{workflow.name}</p>
                        <p className="text-sm text-gray-500">
                          {workflow.id} · {workflow.enabled ? 'enabled' : 'disabled'} · {workflow.steps.length} step{workflow.steps.length === 1 ? '' : 's'}
                        </p>
                        <p className="mt-2 text-sm text-gray-300">{workflow.description}</p>
                        <p className="mt-2 text-sm text-gray-500">
                          {workflow.steps.map((step) => step.name).join(' → ')}
                        </p>
                      </div>
                      <button
                        onClick={() => setForm(cloneWorkflow(workflow))}
                        className="rounded-lg border border-surface px-3 py-2 text-sm hover:border-primary"
                      >
                        Edit
                      </button>
                    </div>
                    <div className="grid gap-3 md:grid-cols-[1fr_auto]">
                      <input
                        value={runInput[workflow.id] ?? ''}
                        onChange={(event) =>
                          setRunInput((current) => ({ ...current, [workflow.id]: event.target.value }))
                        }
                        className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                        placeholder={`Run ${workflow.name} with an input`}
                      />
                      <button
                        onClick={() => void runWorkflow(workflow.id)}
                        className="rounded-lg bg-primary px-4 py-2 text-sm font-medium hover:bg-primary/80"
                      >
                        Run Once
                      </button>
                    </div>
                    {result ? (
                      <div className="rounded-lg border border-surface px-4 py-3 text-sm text-gray-300 space-y-1">
                        <p>Status: {result.status}</p>
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
            <h2 className="text-lg font-semibold">Recent Workflow Runs</h2>
            <p className="text-sm text-gray-400">Workflow runs are persisted alongside task history for restart-safe auditability.</p>
          </div>
          {runs.length === 0 ? (
            <EmptyState text="No workflow runs recorded yet." />
          ) : (
            <div className="space-y-3">
              {runs.map((run) => (
                <div key={run.run_id} className="rounded-lg bg-surface2 p-4">
                  <p className="font-medium">{run.workflow_id}</p>
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

function cloneWorkflow(spec: WorkflowSpec): WorkflowSpec {
  return {
    ...spec,
    steps: spec.steps.map((step) => ({ ...step })),
    tags: [...spec.tags],
  };
}

function normalizeWorkflow(spec: WorkflowSpec): WorkflowSpec {
  return {
    ...spec,
    id: spec.id.trim(),
    name: spec.name.trim(),
    description: spec.description.trim(),
    runtime_profile: emptyToNull(spec.runtime_profile),
    output_contract: emptyToNull(spec.output_contract),
    tags: spec.tags.map((tag) => tag.trim()).filter(Boolean),
    steps: spec.steps
      .map((step, index) => ({
        ...step,
        id: step.id.trim() || `step-${index + 1}`,
        name: step.name.trim() || `Step ${index + 1}`,
        prompt: step.prompt.trim(),
        agent_id: emptyToNull(step.agent_id),
      }))
      .filter((step) => step.prompt),
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
