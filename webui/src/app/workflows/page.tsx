'use client';

import { type ReactNode, useEffect, useState } from 'react';

import Nav from '@/components/Nav';
import {
  AgentSpec,
  DaemonError,
  ExecuteTaskResponse,
  FactoryReview,
  RoveDaemonClient,
  SpecTemplateSummary,
  WorkerPreset,
  WorkflowBranchSpec,
  WorkflowFactoryResult,
  WorkflowRunDetail,
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
      worker_preset: null,
      continue_on_error: false,
      branches: [],
    },
  ],
  channels: [],
  webhooks: [],
  file_watches: [],
  schedules: [],
  tags: [],
};

export default function WorkflowsPage() {
  const [workflows, setWorkflows] = useState<WorkflowSpec[]>([]);
  const [agents, setAgents] = useState<AgentSpec[]>([]);
  const [workerPresets, setWorkerPresets] = useState<WorkerPreset[]>([]);
  const [templates, setTemplates] = useState<SpecTemplateSummary[]>([]);
  const [runs, setRuns] = useState<WorkflowRunRecord[]>([]);
  const [form, setForm] = useState<WorkflowSpec>(EMPTY_WORKFLOW);
  const [factoryRequirement, setFactoryRequirement] = useState('');
  const [factoryTemplate, setFactoryTemplate] = useState('one-shot');
  const [factoryPreview, setFactoryPreview] = useState<WorkflowFactoryResult | null>(null);
  const [formReview, setFormReview] = useState<FactoryReview | null>(null);
  const [runInput, setRunInput] = useState<Record<string, string>>({});
  const [runResult, setRunResult] = useState<Record<string, ExecuteTaskResponse>>({});
  const [runDetails, setRunDetails] = useState<Record<string, WorkflowRunDetail>>({});
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void refresh();
  }, []);

  useEffect(() => {
    if (!form.id || !isDraftSpec(form.provenance)) {
      setFormReview(null);
      return;
    }
    void loadDraftReview(form.id);
  }, [form.id, form.provenance?.draft_for, form.provenance?.review_status]);

  async function refresh() {
    setLoading(true);
    setError(null);
    try {
      const client = daemonClient();
      const [nextWorkflows, nextAgents, nextWorkerPresets, nextRuns, nextTemplates] = await Promise.all([
        client.listWorkflows(),
        client.listAgents(),
        client.listWorkerPresets(),
        client.listWorkflowRuns(),
        client.listWorkflowTemplates(),
      ]);
      setWorkflows(nextWorkflows);
      setAgents(nextAgents);
      setWorkerPresets(nextWorkerPresets);
      setRuns(nextRuns);
      setTemplates(nextTemplates);
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

  async function previewFactory() {
    if (!factoryRequirement.trim()) {
      setError('Factory preview requires a requirement.');
      return;
    }
    setError(null);
    try {
      setFactoryPreview(
        await daemonClient().previewWorkflowFactory({
          requirement: factoryRequirement.trim(),
          template_id: factoryTemplate || undefined,
        }),
      );
    } catch (nextError) {
      setError(formatError(nextError));
    }
  }

  async function createFromFactory() {
    if (!factoryRequirement.trim()) {
      setError('Factory creation requires a requirement.');
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const created = await daemonClient().createWorkflowFactory({
        requirement: factoryRequirement.trim(),
        template_id: factoryTemplate || undefined,
      });
      setFactoryPreview(created);
      setForm(cloneWorkflow(created.spec));
      setFormReview(created.review);
      await refresh();
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setSaving(false);
    }
  }

  async function loadDraftReview(id: string) {
    try {
      setFormReview(await daemonClient().getWorkflowReview(id));
    } catch (nextError) {
      setError(formatError(nextError));
    }
  }

  async function approveDraft() {
    if (!form.id) {
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const approved = await daemonClient().approveWorkflowDraft(form.id);
      setForm(cloneWorkflow(approved));
      setFactoryPreview(null);
      setFormReview(null);
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
      const client = daemonClient();
      const result = await client.runWorkflow(id, input);
      setRunResult((current) => ({ ...current, [id]: result }));
      await refresh();
      if (result.message) {
        const detail = await client.getWorkflowRun(result.message);
        setRunDetails((current) => ({ ...current, [detail.run.run_id]: detail }));
      }
    } catch (nextError) {
      setError(formatError(nextError));
    }
  }

  async function loadRunDetail(runId: string) {
    setError(null);
    try {
      const detail = await daemonClient().getWorkflowRun(runId);
      setRunDetails((current) => ({ ...current, [runId]: detail }));
    } catch (nextError) {
      setError(formatError(nextError));
    }
  }

  async function resumeWorkflowRun(runId: string) {
    setError(null);
    try {
      const client = daemonClient();
      await client.resumeWorkflowRun(runId);
      await refresh();
      const detail = await client.getWorkflowRun(runId);
      setRunDetails((current) => ({ ...current, [runId]: detail }));
    } catch (nextError) {
      setError(formatError(nextError));
    }
  }

  async function cancelWorkflowRun(runId: string) {
    setError(null);
    try {
      const client = daemonClient();
      await client.cancelWorkflowRun(runId);
      await refresh();
      const detail = await client.getWorkflowRun(runId);
      setRunDetails((current) => ({ ...current, [runId]: detail }));
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

  function updateBranch(
    stepIndex: number,
    branchIndex: number,
    patch: Partial<WorkflowBranchSpec>,
  ) {
    setForm((current) => ({
      ...current,
      steps: current.steps.map((step, currentStepIndex) =>
        currentStepIndex === stepIndex
          ? {
              ...step,
              branches: (step.branches ?? []).map((branch, currentBranchIndex) =>
                currentBranchIndex === branchIndex ? { ...branch, ...patch } : branch,
              ),
            }
          : step,
      ),
    }));
  }

  function updateChannel(index: number, patch: Partial<WorkflowSpec['channels'][number]>) {
    setForm((current) => ({
      ...current,
      channels: current.channels.map((binding, bindingIndex) =>
        bindingIndex === index ? { ...binding, ...patch } : binding,
      ),
    }));
  }

  function updateWebhook(index: number, patch: Partial<WorkflowSpec['webhooks'][number]>) {
    setForm((current) => ({
      ...current,
      webhooks: current.webhooks.map((binding, bindingIndex) =>
        bindingIndex === index ? { ...binding, ...patch } : binding,
      ),
    }));
  }

  function updateFileWatch(index: number, patch: Partial<WorkflowSpec['file_watches'][number]>) {
    setForm((current) => ({
      ...current,
      file_watches: current.file_watches.map((binding, bindingIndex) =>
        bindingIndex === index ? { ...binding, ...patch } : binding,
      ),
    }));
  }

  function moveStep(index: number, direction: -1 | 1) {
    setForm((current) => {
      const nextIndex = index + direction;
      if (nextIndex < 0 || nextIndex >= current.steps.length) {
        return current;
      }
      const steps = [...current.steps];
      const [step] = steps.splice(index, 1);
      steps.splice(nextIndex, 0, step);
      return { ...current, steps };
    });
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
          <div>
            <h2 className="text-lg font-semibold">Generate Workflow</h2>
            <p className="text-sm text-gray-400">
              Convert a requirement into a disabled workflow spec with explicit steps, runtime profile, and tags. This stays in the same structured workflow model the daemon already runs.
            </p>
          </div>

          <div className="grid gap-4 md:grid-cols-[220px,1fr]">
            <Field label="Template">
              <select
                value={factoryTemplate}
                onChange={(event) => setFactoryTemplate(event.target.value)}
                className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
              >
                {templates.map((template) => (
                  <option key={template.id} value={template.id}>
                    {template.name}
                  </option>
                ))}
              </select>
            </Field>
            <Field label="Requirement">
              <textarea
                value={factoryRequirement}
                onChange={(event) => setFactoryRequirement(event.target.value)}
                className="min-h-28 w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                placeholder="Inspect the target node, apply the needed fix, then verify the final state."
              />
            </Field>
          </div>

          <div className="flex gap-3">
            <button
              onClick={() => void previewFactory()}
              className="rounded-lg border border-surface px-4 py-2 text-sm hover:border-primary"
            >
              Preview
            </button>
            <button
              onClick={() => void createFromFactory()}
              disabled={saving}
              className="rounded-lg bg-primary px-4 py-2 font-medium hover:bg-primary/80 disabled:bg-surface2 disabled:text-gray-500"
            >
              Create Disabled Workflow
            </button>
          </div>

          {factoryPreview ? (
            <div className="rounded-xl border border-surface bg-background/40 p-4">
              <p className="text-sm text-gray-400">Factory preview</p>
              <FactoryReviewPanel review={factoryPreview.review} />
              <pre className="mt-3 overflow-x-auto text-xs text-gray-300">
                {JSON.stringify(factoryPreview.spec, null, 2)}
              </pre>
            </div>
          ) : null}
        </section>

        <section className="bg-surface rounded-xl p-6 border border-surface2 space-y-5">
          <div className="flex items-center justify-between gap-3">
            <div>
              <h2 className="text-lg font-semibold">Workflow Spec</h2>
              <p className="text-sm text-gray-400">
                Each step can run directly or inherit an agent profile or bounded worker preset. <code>{'{{input}}'}</code>, <code>{'{{last_output}}'}</code>, and prior step outputs like <code>{'{{step_id.result}}'}</code> are available in step prompts.
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
                <option value="edge">edge</option>
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
            <Field label="Schedules">
              <input
                value={formatCsv(form.schedules)}
                onChange={(event) => setForm((current) => ({ ...current, schedules: parseCsv(event.target.value) }))}
                className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                placeholder="0 * * * *, weekdays-09:00"
              />
            </Field>
          </div>

          {formReview ? <FactoryReviewPanel review={formReview} /> : null}

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

          <section className="space-y-4">
            <div>
              <h3 className="font-medium">Triggers</h3>
              <p className="text-sm text-gray-400">
                Bind workflows to inbound channels, public webhooks, or local file changes. Relative file-watch paths resolve from the daemon workspace root.
              </p>
            </div>

            <div className="space-y-3 rounded-lg bg-surface2 p-4">
              <div className="flex items-center justify-between gap-3">
                <div>
                  <h4 className="text-sm font-medium">Channel Bindings</h4>
                  <p className="text-xs text-gray-500">Telegram and plugin-channel ingress can start matching workflows directly.</p>
                </div>
                <button
                  onClick={() =>
                    setForm((current) => ({
                      ...current,
                      channels: [
                        ...current.channels,
                        {
                          kind: '',
                          target: null,
                          enabled: true,
                        },
                      ],
                    }))
                  }
                  className="rounded-lg border border-surface px-3 py-2 text-sm hover:border-primary"
                >
                  Add Channel
                </button>
              </div>
              {form.channels.length === 0 ? (
                <EmptyState text="No channel triggers configured." />
              ) : (
                <div className="space-y-3">
                  {form.channels.map((binding, index) => (
                    <div key={`${binding.kind}-${binding.target ?? index}`} className="rounded-lg border border-surface bg-background/40 p-3 space-y-3">
                      <div className="grid gap-3 md:grid-cols-[1fr_1fr_auto]">
                        <input
                          value={binding.kind}
                          onChange={(event) => updateChannel(index, { kind: event.target.value })}
                          className="rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                          placeholder="telegram"
                        />
                        <input
                          value={binding.target ?? ''}
                          onChange={(event) => updateChannel(index, { target: event.target.value })}
                          className="rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                          placeholder="default or chat:123"
                        />
                        <button
                          onClick={() =>
                            setForm((current) => ({
                              ...current,
                              channels: current.channels.filter((_, bindingIndex) => bindingIndex !== index),
                            }))
                          }
                          className="rounded-lg border border-error/30 px-3 py-2 text-sm text-error hover:bg-error/10"
                        >
                          Remove
                        </button>
                      </div>
                      <Checkbox
                        label="Enabled"
                        checked={binding.enabled}
                        onChange={(checked) => updateChannel(index, { enabled: checked })}
                      />
                    </div>
                  ))}
                </div>
              )}
            </div>

            <div className="space-y-3 rounded-lg bg-surface2 p-4">
              <div className="flex items-center justify-between gap-3">
                <div>
                  <h4 className="text-sm font-medium">Webhook Bindings</h4>
                  <p className="text-xs text-gray-500">
                    Public POST target: <code>/v1/workflows/webhooks/&lt;id&gt;</code>. Optional secret uses the <code>x-rove-webhook-secret</code> header.
                  </p>
                </div>
                <button
                  onClick={() =>
                    setForm((current) => ({
                      ...current,
                      webhooks: [
                        ...current.webhooks,
                        {
                          id: '',
                          secret: null,
                          enabled: true,
                        },
                      ],
                    }))
                  }
                  className="rounded-lg border border-surface px-3 py-2 text-sm hover:border-primary"
                >
                  Add Webhook
                </button>
              </div>
              {form.webhooks.length === 0 ? (
                <EmptyState text="No webhook triggers configured." />
              ) : (
                <div className="space-y-3">
                  {form.webhooks.map((binding, index) => (
                    <div key={`${binding.id}-${index}`} className="rounded-lg border border-surface bg-background/40 p-3 space-y-3">
                      <div className="grid gap-3 md:grid-cols-[1fr_1fr_auto]">
                        <input
                          value={binding.id}
                          onChange={(event) => updateWebhook(index, { id: event.target.value })}
                          className="rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                          placeholder="github-push"
                        />
                        <input
                          value={binding.secret ?? ''}
                          onChange={(event) => updateWebhook(index, { secret: event.target.value })}
                          className="rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                          placeholder="Optional shared secret"
                        />
                        <button
                          onClick={() =>
                            setForm((current) => ({
                              ...current,
                              webhooks: current.webhooks.filter((_, bindingIndex) => bindingIndex !== index),
                            }))
                          }
                          className="rounded-lg border border-error/30 px-3 py-2 text-sm text-error hover:bg-error/10"
                        >
                          Remove
                        </button>
                      </div>
                      <Checkbox
                        label="Enabled"
                        checked={binding.enabled}
                        onChange={(checked) => updateWebhook(index, { enabled: checked })}
                      />
                    </div>
                  ))}
                </div>
              )}
            </div>

            <div className="space-y-3 rounded-lg bg-surface2 p-4">
              <div className="flex items-center justify-between gap-3">
                <div>
                  <h4 className="text-sm font-medium">File-Watch Bindings</h4>
                  <p className="text-xs text-gray-500">
                    Local daemon watcher triggers on create, modify, or remove. Leave events empty to match any supported file event.
                  </p>
                </div>
                <button
                  onClick={() =>
                    setForm((current) => ({
                      ...current,
                      file_watches: [
                        ...current.file_watches,
                        {
                          path: '',
                          recursive: true,
                          events: [],
                          enabled: true,
                        },
                      ],
                    }))
                  }
                  className="rounded-lg border border-surface px-3 py-2 text-sm hover:border-primary"
                >
                  Add File Watch
                </button>
              </div>
              {form.file_watches.length === 0 ? (
                <EmptyState text="No file-watch triggers configured." />
              ) : (
                <div className="space-y-3">
                  {form.file_watches.map((binding, index) => (
                    <div key={`${binding.path}-${index}`} className="rounded-lg border border-surface bg-background/40 p-3 space-y-3">
                      <div className="grid gap-3 md:grid-cols-[1.2fr_1fr_auto]">
                        <input
                          value={binding.path}
                          onChange={(event) => updateFileWatch(index, { path: event.target.value })}
                          className="rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                          placeholder="src or docs/notes.md"
                        />
                        <input
                          value={formatCsv(binding.events ?? [])}
                          onChange={(event) => updateFileWatch(index, { events: parseCsv(event.target.value) })}
                          className="rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                          placeholder="modify, create"
                        />
                        <button
                          onClick={() =>
                            setForm((current) => ({
                              ...current,
                              file_watches: current.file_watches.filter((_, bindingIndex) => bindingIndex !== index),
                            }))
                          }
                          className="rounded-lg border border-error/30 px-3 py-2 text-sm text-error hover:bg-error/10"
                        >
                          Remove
                        </button>
                      </div>
                      <div className="flex flex-wrap gap-2">
                        <Checkbox
                          label="Recursive"
                          checked={binding.recursive}
                          onChange={(checked) => updateFileWatch(index, { recursive: checked })}
                        />
                        <Checkbox
                          label="Enabled"
                          checked={binding.enabled}
                          onChange={(checked) => updateFileWatch(index, { enabled: checked })}
                        />
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </div>

            <div className="space-y-3 rounded-lg bg-surface2 p-4">
              <div>
                <h4 className="text-sm font-medium">Schedules</h4>
                <p className="text-xs text-gray-500">
                  Schedule strings are stored directly on the workflow spec and are visible to the daemon scheduler and import/export flows.
                </p>
              </div>
              <input
                value={formatCsv(form.schedules)}
                onChange={(event) => setForm((current) => ({ ...current, schedules: parseCsv(event.target.value) }))}
                className="rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                placeholder="0 * * * *, nightly-check"
              />
            </div>
          </section>

          <section className="space-y-3">
            <div className="flex items-center justify-between">
              <div>
                <h3 className="font-medium">Steps</h3>
                <p className="text-sm text-gray-400">
                  Steps execute in order unless a branch rule redirects the run to another step by id. Each step may use either an agent profile or a bounded worker preset.
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
                        worker_preset: null,
                        continue_on_error: false,
                        branches: [],
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
                <div className="grid gap-3 md:grid-cols-4">
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
                    onChange={(event) =>
                      updateStep(index, {
                        agent_id: event.target.value || null,
                        worker_preset: event.target.value ? null : step.worker_preset ?? null,
                      })
                    }
                    className="rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                  >
                    <option value="">No agent profile</option>
                    {agents.map((agent) => (
                      <option key={agent.id} value={agent.id}>
                        {agent.name} ({agent.id})
                      </option>
                    ))}
                  </select>
                  <select
                    value={step.worker_preset ?? ''}
                    onChange={(event) =>
                      updateStep(index, {
                        worker_preset: event.target.value || null,
                        agent_id: event.target.value ? null : step.agent_id ?? null,
                      })
                    }
                    className="rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                  >
                    <option value="">No worker preset</option>
                    {workerPresets.map((preset) => (
                      <option key={preset.id} value={preset.id}>
                        {preset.name} ({preset.id})
                      </option>
                    ))}
                  </select>
                </div>
                <textarea
                  value={step.prompt}
                  onChange={(event) => updateStep(index, { prompt: event.target.value })}
                  className="min-h-28 w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                  placeholder="Use {{input}}, {{last_output}}, and {{step_id.result}} when needed"
                />
                <div className="flex items-center justify-between gap-3">
                  <Checkbox
                    label="Continue On Error"
                    checked={step.continue_on_error}
                    onChange={(checked) => updateStep(index, { continue_on_error: checked })}
                  />
                  <div className="flex flex-wrap gap-2">
                    <button
                      onClick={() => moveStep(index, -1)}
                      disabled={index === 0}
                      className="rounded-lg border border-surface px-3 py-2 text-sm hover:border-primary disabled:cursor-not-allowed disabled:opacity-50"
                    >
                      Move Up
                    </button>
                    <button
                      onClick={() => moveStep(index, 1)}
                      disabled={index === form.steps.length - 1}
                      className="rounded-lg border border-surface px-3 py-2 text-sm hover:border-primary disabled:cursor-not-allowed disabled:opacity-50"
                    >
                      Move Down
                    </button>
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
                <div className="space-y-3 rounded-lg border border-surface bg-background/40 p-3">
                  <div className="flex items-center justify-between gap-3">
                    <div>
                      <p className="text-sm font-medium">Branch Rules</p>
                      <p className="text-xs text-gray-500">
                        First case-insensitive <code>contains</code> match wins. If no rule matches, the workflow continues to the next step.
                      </p>
                    </div>
                    <button
                      onClick={() =>
                        updateStep(index, {
                          branches: [
                            ...(step.branches ?? []),
                            {
                              contains: '',
                              next_step_id: '',
                            },
                          ],
                        })
                      }
                      className="rounded-lg border border-surface px-3 py-2 text-sm hover:border-primary"
                    >
                      Add Branch
                    </button>
                  </div>
                  {(step.branches ?? []).length === 0 ? (
                    <EmptyState text="No conditional branches. The next step stays linear." />
                  ) : (
                    <div className="space-y-3">
                      {(step.branches ?? []).map((branch, branchIndex) => (
                        <div
                          key={`${step.id || index}-branch-${branchIndex}`}
                          className="grid gap-3 md:grid-cols-[1fr_1fr_auto]"
                        >
                          <input
                            value={branch.contains}
                            onChange={(event) =>
                              updateBranch(index, branchIndex, { contains: event.target.value })
                            }
                            className="rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                            placeholder="retry"
                          />
                          <input
                            value={branch.next_step_id}
                            onChange={(event) =>
                              updateBranch(index, branchIndex, { next_step_id: event.target.value })
                            }
                            className="rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                            placeholder="fix-step"
                          />
                          <button
                            onClick={() =>
                              updateStep(index, {
                                branches: (step.branches ?? []).filter(
                                  (_, currentBranchIndex) => currentBranchIndex !== branchIndex,
                                ),
                              })
                            }
                            className="rounded-lg border border-error/30 px-3 py-2 text-sm text-error hover:bg-error/10"
                          >
                            Remove
                          </button>
                        </div>
                      ))}
                    </div>
                  )}
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
              {saving ? 'Saving…' : isDraftSpec(form.provenance) ? 'Save Draft' : 'Save Workflow'}
            </button>
            {isDraftSpec(form.provenance) ? (
              <button
                onClick={() => void approveDraft()}
                disabled={saving}
                className="rounded-lg border border-primary/40 px-4 py-2 text-sm hover:bg-primary/10 disabled:cursor-not-allowed disabled:opacity-60"
              >
                Approve Draft
              </button>
            ) : null}
            {form.id ? (
              <button
                onClick={() => void removeWorkflow(form.id)}
                className="rounded-lg border border-error/30 px-4 py-2 text-sm text-error hover:bg-error/10"
              >
                Remove
              </button>
            ) : null}
          </div>

          <SpecMetadataPanel provenance={form.provenance} snapshot={normalizeWorkflow(form)} />

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
                        {isDraftSpec(workflow.provenance) ? (
                          <p className="mt-1 text-xs text-warning">
                            draft for {workflow.provenance?.draft_for ?? workflow.id}
                          </p>
                        ) : null}
                        <p className="mt-2 text-sm text-gray-300">{workflow.description}</p>
                        <p className="mt-2 text-sm text-gray-500">
                          {workflow.steps.map((step) => step.name).join(' → ')}
                        </p>
                        <div className="mt-3 flex flex-wrap gap-2 text-xs text-gray-400">
                          {workflow.channels.length > 0 ? (
                            <span className="rounded-full border border-surface px-2 py-1">
                              channels: {workflow.channels.map(formatChannelBinding).join(' · ')}
                            </span>
                          ) : null}
                          {workflow.webhooks.length > 0 ? (
                            <span className="rounded-full border border-surface px-2 py-1">
                              webhooks: {workflow.webhooks.map(formatWebhookBinding).join(' · ')}
                            </span>
                          ) : null}
                          {workflow.file_watches.length > 0 ? (
                            <span className="rounded-full border border-surface px-2 py-1">
                              file watch: {workflow.file_watches.map(formatFileWatchBinding).join(' · ')}
                            </span>
                          ) : null}
                          {workflow.schedules.length > 0 ? (
                            <span className="rounded-full border border-surface px-2 py-1">
                              schedules: {workflow.schedules.join(' · ')}
                            </span>
                          ) : null}
                        </div>
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
                  <p className="mt-1 text-sm text-gray-400">
                    Progress: {run.steps_completed}/{run.steps_total} steps
                    {run.current_step_name ? ` · current: ${run.current_step_name}` : ''}
                    {run.retry_count > 0 ? ` · retries: ${run.retry_count}` : ''}
                    {run.cancel_requested ? ' · cancel requested' : ''}
                  </p>
                  <p className="mt-2 text-sm text-gray-300 whitespace-pre-wrap">{run.input}</p>
                  {run.output ? <p className="mt-2 text-sm text-gray-400 whitespace-pre-wrap">{run.output}</p> : null}
                  {run.error ? <p className="mt-2 text-sm text-error whitespace-pre-wrap">{run.error}</p> : null}
                  <div className="mt-3 flex flex-wrap gap-2">
                    {run.resumable ? (
                      <button
                        onClick={() => void resumeWorkflowRun(run.run_id)}
                        className="rounded-lg border border-primary/40 px-3 py-2 text-sm hover:bg-primary/10"
                      >
                        {run.status === 'failed' ? 'Retry From Failed Step' : 'Resume Run'}
                      </button>
                    ) : null}
                    {run.status === 'running' && !run.cancel_requested ? (
                      <button
                        onClick={() => void cancelWorkflowRun(run.run_id)}
                        className="rounded-lg border border-error/30 px-3 py-2 text-sm text-error hover:bg-error/10"
                      >
                        Cancel Run
                      </button>
                    ) : null}
                    <button
                      onClick={() => void loadRunDetail(run.run_id)}
                      className="rounded-lg border border-surface px-3 py-2 text-sm hover:border-primary"
                    >
                      View Steps
                    </button>
                  </div>
                  {runDetails[run.run_id] ? (
                    <div className="mt-3 space-y-2 rounded-lg border border-surface bg-background/40 p-3">
                      {Object.keys(runDetails[run.run_id].variables ?? {}).length > 0 ? (
                        <div className="rounded-lg border border-surface px-3 py-2">
                          <p className="text-sm font-medium">Variables</p>
                          <div className="mt-2 space-y-1">
                            {Object.entries(runDetails[run.run_id].variables)
                              .sort(([left], [right]) => left.localeCompare(right))
                              .map(([name, value]) => (
                                <div key={name}>
                                  <p className="text-xs text-gray-500">{name}</p>
                                  <p className="text-xs text-gray-300 whitespace-pre-wrap">{value}</p>
                                </div>
                              ))}
                          </div>
                        </div>
                      ) : null}
                      {runDetails[run.run_id].steps.map((step) => (
                        <div key={`${step.run_id}-${step.step_index}`} className="rounded-lg border border-surface px-3 py-2">
                          <p className="text-sm font-medium">
                            {step.step_index + 1}. {step.step_name} · {step.status}
                          </p>
                          <p className="text-xs text-gray-500">
                            {step.agent_id ? `agent:${step.agent_id}` : step.worker_preset ? `worker:${step.worker_preset}` : 'direct'}
                            {step.attempt_count > 1 ? ` · attempts:${step.attempt_count}` : ''}
                            {step.task_id ? ` · task:${step.task_id}` : ''}
                          </p>
                          <p className="mt-2 text-xs text-gray-400 whitespace-pre-wrap">{step.prompt}</p>
                          {step.output ? (
                            <p className="mt-2 text-xs text-gray-300 whitespace-pre-wrap">{step.output}</p>
                          ) : null}
                          {step.error ? (
                            <p className="mt-2 text-xs text-error whitespace-pre-wrap">{step.error}</p>
                          ) : null}
                        </div>
                      ))}
                    </div>
                  ) : null}
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
    steps: spec.steps.map((step) => ({
      ...step,
      worker_preset: step.worker_preset ?? null,
      branches: [...(step.branches ?? [])],
    })),
    channels: (spec.channels ?? []).map((binding) => ({ ...binding, target: binding.target ?? null })),
    webhooks: (spec.webhooks ?? []).map((binding) => ({ ...binding, secret: binding.secret ?? null })),
    file_watches: (spec.file_watches ?? []).map((binding) => ({
      ...binding,
      events: [...(binding.events ?? [])],
    })),
    schedules: [...(spec.schedules ?? [])],
    tags: [...(spec.tags ?? [])],
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
    channels: (spec.channels ?? []).map((binding) => ({
      ...binding,
      kind: binding.kind.trim(),
      target: emptyToNull(binding.target),
    })),
    webhooks: (spec.webhooks ?? []).map((binding) => ({
      ...binding,
      id: binding.id.trim(),
      secret: emptyToNull(binding.secret),
    })),
    file_watches: (spec.file_watches ?? [])
      .map((binding) => ({
        ...binding,
        path: binding.path.trim(),
        events: (binding.events ?? []).map((event) => event.trim()).filter(Boolean),
      }))
      .filter((binding) => binding.path),
    schedules: (spec.schedules ?? []).map((value) => value.trim()).filter(Boolean),
    tags: (spec.tags ?? []).map((tag) => tag.trim()).filter(Boolean),
    steps: spec.steps
      .map((step, index) => ({
        ...step,
        id: step.id.trim() || `step-${index + 1}`,
        name: step.name.trim() || `Step ${index + 1}`,
        prompt: step.prompt.trim(),
        agent_id: emptyToNull(step.agent_id),
        worker_preset: emptyToNull(step.worker_preset),
        branches: (step.branches ?? [])
          .map((branch) => ({
            contains: branch.contains.trim(),
            next_step_id: branch.next_step_id.trim(),
          }))
          .filter((branch) => branch.contains && branch.next_step_id),
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

function formatChannelBinding(binding: WorkflowSpec['channels'][number]) {
  const target = binding.target?.trim() ? binding.target.trim() : '*';
  return `${binding.kind}:${target}${binding.enabled ? '' : ' (disabled)'}`;
}

function formatWebhookBinding(binding: WorkflowSpec['webhooks'][number]) {
  return `${binding.id}${binding.secret ? ' [secret]' : ''}${binding.enabled ? '' : ' (disabled)'}`;
}

function formatFileWatchBinding(binding: WorkflowSpec['file_watches'][number]) {
  const events = binding.events.length > 0 ? binding.events.join(',') : 'any';
  return `${binding.path}${binding.recursive ? ' [recursive]' : ''} [${events}]${binding.enabled ? '' : ' (disabled)'}`;
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

function isDraftSpec(provenance?: WorkflowSpec['provenance']) {
  return provenance?.review_status === 'draft' || Boolean(provenance?.draft_for);
}

function SpecMetadataPanel({
  provenance,
  snapshot,
}: {
  provenance?: WorkflowSpec['provenance'];
  snapshot: WorkflowSpec;
}) {
  return (
    <section className="space-y-3 rounded-xl border border-surface bg-background/30 p-4">
      <div>
        <h3 className="font-medium">Spec Metadata</h3>
        <p className="text-sm text-gray-400">
          This editor writes the workflow spec the daemon persists as TOML. Provenance and review state stay visible so branch and trigger edits remain auditable.
        </p>
      </div>
      {provenance ? (
        <div className="grid gap-3 md:grid-cols-2">
          <MetaValue label="Source" value={provenance.source} />
          <MetaValue label="Import Source" value={provenance.import_source} />
          <MetaValue label="Draft For" value={provenance.draft_for} />
          <MetaValue label="Review Status" value={provenance.review_status} />
          <MetaValue label="Notes" value={provenance.notes} />
          <MetaValue label="Reviewed At" value={formatOptionalTimestamp(provenance.reviewed_at)} />
        </div>
      ) : (
        <EmptyState text="No provenance metadata recorded for this workflow." />
      )}
      <div>
        <p className="text-xs uppercase tracking-wide text-gray-500">Current Snapshot</p>
        <pre className="mt-2 max-h-72 overflow-auto rounded-lg border border-surface bg-surface2 p-3 text-xs text-gray-300">
          {JSON.stringify(snapshot, null, 2)}
        </pre>
      </div>
    </section>
  );
}

function FactoryReviewPanel({ review }: { review: FactoryReview }) {
  return (
    <div className="mt-3 space-y-3 rounded-lg border border-surface bg-surface2/60 p-4">
      <div>
        <p className="text-sm font-medium">
          {review.kind} review · {review.review_status}
        </p>
        <p className="text-sm text-gray-400">{review.summary}</p>
        <p className="text-xs text-gray-500">
          target {review.target_id}
          {review.draft_id ? ` · draft ${review.draft_id}` : ''}
          {review.target_exists ? ' · existing target' : ' · new target'}
        </p>
      </div>
      {review.warnings.length > 0 ? (
        <div className="space-y-2">
          <p className="text-xs uppercase tracking-wide text-warning">Warnings</p>
          {review.warnings.map((warning) => (
            <p key={warning} className="text-sm text-warning">
              {warning}
            </p>
          ))}
        </div>
      ) : null}
      {review.changes.length > 0 ? (
        <div className="space-y-2">
          <p className="text-xs uppercase tracking-wide text-gray-500">Changes</p>
          <div className="space-y-2">
            {review.changes.slice(0, 12).map((change) => (
              <div key={change.field} className="rounded-lg border border-surface px-3 py-2 text-xs">
                <p className="font-medium text-gray-300">{change.field}</p>
                <p className="mt-1 text-gray-500">current: {change.current ?? 'unset'}</p>
                <p className="text-gray-400">proposed: {change.proposed ?? 'unset'}</p>
              </div>
            ))}
            {review.changes.length > 12 ? (
              <p className="text-xs text-gray-500">
                {review.changes.length - 12} more change{review.changes.length - 12 === 1 ? '' : 's'}
              </p>
            ) : null}
          </div>
        </div>
      ) : null}
    </div>
  );
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

function MetaValue({ label, value }: { label: string; value?: string | null }) {
  return (
    <div className="rounded-lg border border-surface px-3 py-2">
      <p className="text-xs uppercase tracking-wide text-gray-500">{label}</p>
      <p className="mt-1 text-sm text-gray-300 whitespace-pre-wrap">{value?.trim() ? value : 'unset'}</p>
    </div>
  );
}

function formatOptionalTimestamp(value?: number | null) {
  return value ? formatTimestamp(value) : null;
}
