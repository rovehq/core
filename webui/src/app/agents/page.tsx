'use client';

import { type ReactNode, useEffect, useState } from 'react';

import Nav from '@/components/Nav';
import {
  AgentSpec,
  AgentFactoryResult,
  DaemonError,
  ExecuteTaskResponse,
  FactoryReview,
  RoveDaemonClient,
  AgentRunRecord,
  SpecTemplateSummary,
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
  outcome_contract: null,
  ui: {},
  tags: [],
};

export default function AgentsPage() {
  const [agents, setAgents] = useState<AgentSpec[]>([]);
  const [runs, setRuns] = useState<AgentRunRecord[]>([]);
  const [templates, setTemplates] = useState<SpecTemplateSummary[]>([]);
  const [form, setForm] = useState<AgentSpec>(EMPTY_AGENT);
  const [factoryRequirement, setFactoryRequirement] = useState('');
  const [factoryTemplate, setFactoryTemplate] = useState('general-assistant');
  const [factoryPreview, setFactoryPreview] = useState<AgentFactoryResult | null>(null);
  const [formReview, setFormReview] = useState<FactoryReview | null>(null);
  const [runInput, setRunInput] = useState<Record<string, string>>({});
  const [runResult, setRunResult] = useState<Record<string, ExecuteTaskResponse>>({});
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
      const [nextAgents, nextRuns, nextTemplates] = await Promise.all([
        client.listAgents(),
        client.listAgentRuns(),
        client.listAgentTemplates(),
      ]);
      setAgents(nextAgents);
      setRuns(nextRuns);
      setTemplates(nextTemplates);
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

  async function previewFactory() {
    if (!factoryRequirement.trim()) {
      setError('Factory preview requires a requirement.');
      return;
    }

    setError(null);
    try {
      setFactoryPreview(
        await daemonClient().previewAgentFactory({
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
      const created = await daemonClient().createAgentFactory({
        requirement: factoryRequirement.trim(),
        template_id: factoryTemplate || undefined,
      });
      setFactoryPreview(created);
      setForm(cloneAgent(created.spec));
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
      setFormReview(await daemonClient().getAgentReview(id));
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
      const approved = await daemonClient().approveAgentDraft(form.id);
      setForm(cloneAgent(approved));
      setFactoryPreview(null);
      setFormReview(null);
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

  function updateChannel(index: number, patch: Partial<AgentSpec['channels'][number]>) {
    setForm((current) => ({
      ...current,
      channels: current.channels.map((binding, bindingIndex) =>
        bindingIndex === index ? { ...binding, ...patch } : binding,
      ),
    }));
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
          <div>
            <h2 className="text-lg font-semibold">Generate Agent</h2>
            <p className="text-sm text-gray-400">
              Turn a requirement into a disabled, explicit agent spec. Generation stays reviewable: instructions, capabilities, channels, approval mode, and placement all land as normal structured fields.
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
                placeholder="Create a Telegram support agent that can read files, inspect logs, and run safe operational commands."
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
              Create Disabled Agent
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
                <option value="edge">edge</option>
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
            <Field label="Model Policy">
              <input
                value={form.model_policy ?? ''}
                onChange={(event) =>
                  setForm((current) => ({
                    ...current,
                    model_policy: event.target.value || null,
                  }))
                }
                className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                placeholder="Optional provider/model routing policy"
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

          <section className="space-y-3 rounded-xl border border-surface bg-background/30 p-4">
            <div className="flex items-center justify-between gap-3">
              <div>
                <h3 className="font-medium">Outcome Contract</h3>
                <p className="text-sm text-gray-400">
                  Add bounded self-evaluation after the first answer. The agent will retry only up to the configured budget.
                </p>
              </div>
              <button
                onClick={() =>
                  setForm((current) => ({
                    ...current,
                    outcome_contract: current.outcome_contract ?? {
                      success_criteria: '',
                      max_self_evals: 1,
                      evaluator_policy: 'self_check',
                    },
                  }))
                }
                className="rounded-lg border border-surface px-3 py-2 text-sm hover:border-primary"
              >
                {form.outcome_contract ? 'Configured' : 'Enable'}
              </button>
            </div>

            {form.outcome_contract ? (
              <div className="space-y-4">
                <Field label="Success Criteria">
                  <textarea
                    value={form.outcome_contract.success_criteria}
                    onChange={(event) =>
                      setForm((current) => ({
                        ...current,
                        outcome_contract: {
                          ...(current.outcome_contract ?? {
                            success_criteria: '',
                            max_self_evals: 1,
                            evaluator_policy: 'self_check',
                          }),
                          success_criteria: event.target.value,
                        },
                      }))
                    }
                    className="min-h-24 w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                    placeholder="State what must be true for the answer to count as complete."
                  />
                </Field>
                <div className="grid gap-4 md:grid-cols-2">
                  <Field label="Evaluator Policy">
                    <select
                      value={form.outcome_contract.evaluator_policy}
                      onChange={(event) =>
                        setForm((current) => ({
                          ...current,
                          outcome_contract: {
                            ...(current.outcome_contract ?? {
                              success_criteria: '',
                              max_self_evals: 1,
                              evaluator_policy: 'self_check',
                            }),
                            evaluator_policy: event.target.value,
                          },
                        }))
                      }
                      className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                    >
                      <option value="self_check">self_check</option>
                      <option value="verifier_strict">verifier_strict</option>
                    </select>
                  </Field>
                  <Field label="Max Self-Evals">
                    <input
                      type="number"
                      min={0}
                      max={8}
                      value={form.outcome_contract.max_self_evals}
                      onChange={(event) =>
                        setForm((current) => ({
                          ...current,
                          outcome_contract: {
                            ...(current.outcome_contract ?? {
                              success_criteria: '',
                              max_self_evals: 1,
                              evaluator_policy: 'self_check',
                            }),
                            max_self_evals: Math.max(0, Number.parseInt(event.target.value || '0', 10) || 0),
                          },
                        }))
                      }
                      className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                    />
                  </Field>
                </div>
                <button
                  onClick={() => setForm((current) => ({ ...current, outcome_contract: null }))}
                  className="rounded-lg border border-error/30 px-3 py-2 text-sm text-error hover:bg-error/10"
                >
                  Remove Outcome Contract
                </button>
              </div>
            ) : (
              <EmptyState text="No outcome contract configured. The agent will stay single-pass." />
            )}
          </section>

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
                <h3 className="font-medium">Channel Bindings</h3>
                <p className="text-sm text-gray-400">
                  Bind the agent directly to inbound channel targets. These bindings are stored on the spec and reused by channel surfaces.
                </p>
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
              <EmptyState text="No channel bindings configured." />
            ) : (
              <div className="space-y-3">
                {form.channels.map((binding, index) => (
                  <div key={`${binding.kind}-${binding.target ?? index}`} className="grid gap-3 rounded-lg bg-surface2 p-4 md:grid-cols-[1fr_1fr_auto_auto]">
                    <input
                      value={binding.kind}
                      onChange={(event) => updateChannel(index, { kind: event.target.value })}
                      className="rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                      placeholder="telegram"
                    />
                    <input
                      value={binding.target ?? ''}
                      onChange={(event) => updateChannel(index, { target: event.target.value || null })}
                      className="rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                      placeholder="default or chat:123"
                    />
                    <Checkbox
                      label="Enabled"
                      checked={binding.enabled}
                      onChange={(checked) => updateChannel(index, { enabled: checked })}
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
                ))}
              </div>
            )}
          </section>

          <section className="grid gap-4 md:grid-cols-2">
            <Field label="UI Icon">
              <input
                value={form.ui.icon ?? ''}
                onChange={(event) =>
                  setForm((current) => ({
                    ...current,
                    ui: { ...current.ui, icon: event.target.value || null },
                  }))
                }
                className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                placeholder="◎"
              />
            </Field>
            <Field label="UI Accent">
              <input
                value={form.ui.accent ?? ''}
                onChange={(event) =>
                  setForm((current) => ({
                    ...current,
                    ui: { ...current.ui, accent: event.target.value || null },
                  }))
                }
                className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                placeholder="primary"
              />
            </Field>
          </section>

          <SpecMetadataPanel
            provenance={form.provenance}
            snapshot={normalizeAgent(form)}
          />

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
              {saving ? 'Saving…' : isDraftSpec(form.provenance) ? 'Save Draft' : 'Save Agent'}
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
                        {isDraftSpec(agent.provenance) ? (
                          <p className="mt-1 text-xs text-warning">
                            draft for {agent.provenance?.draft_for ?? agent.id}
                          </p>
                        ) : null}
                        <p className="mt-2 text-sm text-gray-300">{agent.purpose}</p>
                        <p className="mt-2 text-sm text-gray-500">
                          capabilities {capabilityNames.join(', ') || 'none'}
                        </p>
                        {agent.channels.length > 0 ? (
                          <p className="mt-1 text-sm text-gray-500">
                            channels {agent.channels.map(formatChannelBinding).join(', ')}
                          </p>
                        ) : null}
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
    outcome_contract: spec.outcome_contract ? { ...spec.outcome_contract } : null,
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
    outcome_contract:
      spec.outcome_contract?.success_criteria.trim()
        ? {
            success_criteria: spec.outcome_contract.success_criteria.trim(),
            max_self_evals: Math.max(0, spec.outcome_contract.max_self_evals ?? 0),
            evaluator_policy: spec.outcome_contract.evaluator_policy.trim() || 'self_check',
          }
        : null,
    channels: spec.channels
      .map((binding) => ({
        ...binding,
        kind: binding.kind.trim(),
        target: emptyToNull(binding.target),
      }))
      .filter((binding) => binding.kind),
    schedules: spec.schedules.map((value) => value.trim()).filter(Boolean),
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

function formatChannelBinding(binding: AgentSpec['channels'][number]) {
  const target = binding.target?.trim() ? binding.target.trim() : '*';
  return `${binding.kind}:${target}${binding.enabled ? '' : ' (disabled)'}`;
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

function isDraftSpec(provenance?: AgentSpec['provenance']) {
  return provenance?.review_status === 'draft' || Boolean(provenance?.draft_for);
}

function SpecMetadataPanel({
  provenance,
  snapshot,
}: {
  provenance?: AgentSpec['provenance'];
  snapshot: AgentSpec;
}) {
  return (
    <section className="space-y-3 rounded-xl border border-surface bg-background/30 p-4">
      <div>
        <h3 className="font-medium">Spec Metadata</h3>
        <p className="text-sm text-gray-400">
          This form writes the daemon-backed TOML spec. Draft provenance stays visible here so review state is not hidden behind file edits.
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
        <EmptyState text="No provenance metadata recorded for this spec." />
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
