'use client';

import Link from 'next/link';
import { useEffect, useMemo, useState } from 'react';

import Nav from '@/components/Nav';
import {
  AgentFactoryResult,
  DaemonError,
  RoveDaemonClient,
  StarterCatalogEntry,
  StarterCatalogKind,
  WorkflowFactoryResult,
  readStoredToken,
} from '@/lib/daemon';

const KIND_ORDER: StarterCatalogKind[] = [
  'agent_template',
  'workflow_template',
  'worker_preset',
  'channel_starter',
  'capability_pack',
];

const KIND_LABELS: Record<StarterCatalogKind, string> = {
  agent_template: 'Agent Templates',
  workflow_template: 'Workflow Templates',
  worker_preset: 'Worker Presets',
  channel_starter: 'Channel Starters',
  capability_pack: 'Capability Packs',
};

export default function StartersPage() {
  const [entries, setEntries] = useState<StarterCatalogEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [requirements, setRequirements] = useState<Record<string, string>>({});
  const [previews, setPreviews] = useState<Record<string, AgentFactoryResult | WorkflowFactoryResult>>({});
  const [messages, setMessages] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<Record<string, 'preview' | 'create'>>({});

  useEffect(() => {
    void refresh();
  }, []);

  const grouped = useMemo(() => {
    return KIND_ORDER.map((kind) => ({
      kind,
      label: KIND_LABELS[kind],
      entries: entries.filter((entry) => entry.kind === kind),
    })).filter((group) => group.entries.length > 0);
  }, [entries]);

  async function refresh() {
    setLoading(true);
    setError(null);
    try {
      const nextEntries = await daemonClient().listStarters();
      setEntries(nextEntries);
      setRequirements((current) => {
        const next = { ...current };
        for (const entry of nextEntries) {
          if (supportsDraftFactory(entry.kind) && !next[entry.id]) {
            next[entry.id] = defaultRequirement(entry);
          }
        }
        return next;
      });
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setLoading(false);
    }
  }

  async function previewTemplate(entry: StarterCatalogEntry) {
    const requirement = (requirements[entry.id] ?? defaultRequirement(entry)).trim();
    if (!requirement) {
      setMessages((current) => ({ ...current, [entry.id]: 'Requirement is required for preview.' }));
      return;
    }

    setBusy((current) => ({ ...current, [entry.id]: 'preview' }));
    setMessages((current) => ({ ...current, [entry.id]: '' }));
    try {
      const client = daemonClient();
      const preview =
        entry.kind === 'agent_template'
          ? await client.previewAgentFactory({
              requirement,
              template_id: templateId(entry),
            })
          : await client.previewWorkflowFactory({
              requirement,
              template_id: templateId(entry),
            });
      setPreviews((current) => ({ ...current, [entry.id]: preview }));
    } catch (nextError) {
      setMessages((current) => ({ ...current, [entry.id]: formatError(nextError) }));
    } finally {
      setBusy((current) => {
        const next = { ...current };
        delete next[entry.id];
        return next;
      });
    }
  }

  async function createDraft(entry: StarterCatalogEntry) {
    const requirement = (requirements[entry.id] ?? defaultRequirement(entry)).trim();
    if (!requirement) {
      setMessages((current) => ({ ...current, [entry.id]: 'Requirement is required to create a draft.' }));
      return;
    }

    setBusy((current) => ({ ...current, [entry.id]: 'create' }));
    setMessages((current) => ({ ...current, [entry.id]: '' }));
    try {
      const client = daemonClient();
      const created =
        entry.kind === 'agent_template'
          ? await client.createAgentFactory({
              requirement,
              template_id: templateId(entry),
            })
          : await client.createWorkflowFactory({
              requirement,
              template_id: templateId(entry),
            });
      setPreviews((current) => ({ ...current, [entry.id]: created }));
      setMessages((current) => ({
        ...current,
        [entry.id]:
          entry.kind === 'agent_template'
            ? `Draft saved as ${created.spec.id}. Open /agents to review and approve it.`
            : `Draft saved as ${created.spec.id}. Open /workflows to review and approve it.`,
      }));
    } catch (nextError) {
      setMessages((current) => ({ ...current, [entry.id]: formatError(nextError) }));
    } finally {
      setBusy((current) => {
        const next = { ...current };
        delete next[entry.id];
        return next;
      });
    }
  }

  return (
    <div className="min-h-screen flex flex-col">
      <header className="sticky top-0 z-10 bg-background/80 backdrop-blur border-b border-surface2">
        <div className="max-w-6xl mx-auto px-4 py-4 space-y-4">
          <div>
            <h1 className="text-2xl font-semibold">Starter Catalog</h1>
            <p className="text-sm text-gray-400">
              Official starting points for agents, workflows, worker presets, channels, and capability packs. This stays curated and daemon-native instead of turning into an unbounded marketplace shell.
            </p>
          </div>
          <Nav />
        </div>
      </header>

      <main className="flex-1 max-w-6xl w-full mx-auto px-4 py-6 space-y-6">
        <section className="bg-surface rounded-xl p-6 border border-surface2 flex flex-wrap items-center justify-between gap-4">
          <div className="space-y-2">
            <h2 className="text-lg font-semibold">Official Starter Surface</h2>
            <p className="text-sm text-gray-400 max-w-3xl">
              Use this catalog to seed structured specs and approved setup flows. One-click trusted capability-pack install is a separate follow-up; today the catalog points to the right reviewed flows instead of inventing hidden automation.
            </p>
          </div>
          <button
            onClick={() => void refresh()}
            className="rounded-lg border border-surface2 px-4 py-2 text-sm hover:border-primary"
          >
            Refresh
          </button>
        </section>

        {error ? (
          <section className="rounded-xl border border-red-500/40 bg-red-500/10 px-4 py-3 text-sm text-red-200">
            {error}
          </section>
        ) : null}

        {loading ? (
          <section className="rounded-xl border border-surface2 bg-surface px-4 py-6 text-sm text-gray-400">
            Loading starter catalog...
          </section>
        ) : null}

        {!loading &&
          grouped.map((group) => (
            <section key={group.kind} className="space-y-4">
              <div>
                <h2 className="text-lg font-semibold">{group.label}</h2>
                <p className="text-sm text-gray-400">
                  {sectionCopy(group.kind)}
                </p>
              </div>
              <div className="grid gap-4 lg:grid-cols-2">
                {group.entries.map((entry) => (
                  <article
                    key={entry.id}
                    className="rounded-xl border border-surface2 bg-surface p-5 space-y-4"
                  >
                    <div className="flex flex-wrap items-start justify-between gap-3">
                      <div className="space-y-2">
                        <div className="flex flex-wrap items-center gap-2">
                          <h3 className="text-base font-semibold">{entry.name}</h3>
                          <StatusBadge status={entry.status} />
                          <TrustBadge official={entry.official} />
                        </div>
                        <p className="text-sm text-gray-400">{entry.description}</p>
                      </div>
                    </div>

                    {entry.tags.length > 0 ? (
                      <div className="flex flex-wrap gap-2">
                        {entry.tags.map((tag) => (
                          <span
                            key={`${entry.id}:${tag}`}
                            className="rounded-full border border-surface2 bg-background/50 px-2 py-0.5 text-xs text-gray-300"
                          >
                            {tag}
                          </span>
                        ))}
                      </div>
                    ) : null}

                    {entry.components.length > 0 ? (
                      <div className="space-y-1">
                        <p className="text-xs uppercase tracking-[0.2em] text-gray-500">Components</p>
                        <p className="text-sm text-gray-300">{entry.components.join(', ')}</p>
                      </div>
                    ) : null}

                    {entry.notes.length > 0 ? (
                      <div className="space-y-1">
                        <p className="text-xs uppercase tracking-[0.2em] text-gray-500">Operator Notes</p>
                        <ul className="space-y-1 text-sm text-gray-300">
                          {entry.notes.slice(0, 3).map((note) => (
                            <li key={`${entry.id}:${note}`}>- {note}</li>
                          ))}
                        </ul>
                      </div>
                    ) : null}

                    <div className="space-y-3 pt-2">
                      <div className="flex flex-wrap gap-3">
                        {entry.action_route ? (
                          <Link
                            href={entry.action_route}
                            className="inline-flex rounded-lg bg-primary px-4 py-2 text-sm font-medium text-white hover:bg-primary/80"
                          >
                            {entry.action_label}
                          </Link>
                        ) : (
                          <div className="inline-flex rounded-lg border border-surface2 px-4 py-2 text-sm text-gray-200">
                            {entry.action_label}
                          </div>
                        )}
                        {supportsDraftFactory(entry.kind) ? (
                          <>
                            <button
                              onClick={() => void previewTemplate(entry)}
                              disabled={busy[entry.id] !== undefined}
                              className="rounded-lg border border-surface2 px-4 py-2 text-sm text-gray-200 hover:border-primary disabled:cursor-not-allowed disabled:opacity-60"
                            >
                              {busy[entry.id] === 'preview' ? 'Previewing…' : 'Preview draft'}
                            </button>
                            <button
                              onClick={() => void createDraft(entry)}
                              disabled={busy[entry.id] !== undefined}
                              className="rounded-lg border border-primary/30 bg-primary/10 px-4 py-2 text-sm text-primary hover:bg-primary/20 disabled:cursor-not-allowed disabled:opacity-60"
                            >
                              {busy[entry.id] === 'create' ? 'Saving…' : 'Create draft'}
                            </button>
                          </>
                        ) : null}
                      </div>

                      {supportsDraftFactory(entry.kind) ? (
                        <div className="space-y-2 rounded-lg border border-surface2 bg-background/60 px-3 py-3">
                          <p className="text-xs uppercase tracking-[0.2em] text-gray-500">Customize</p>
                          <textarea
                            value={requirements[entry.id] ?? defaultRequirement(entry)}
                            onChange={(event) =>
                              setRequirements((current) => ({
                                ...current,
                                [entry.id]: event.target.value,
                              }))
                            }
                            rows={3}
                            className="w-full rounded-lg border border-surface2 bg-background px-3 py-2 text-sm text-gray-100 outline-none focus:border-primary"
                          />
                          <p className="text-xs text-gray-500">
                            This requirement is passed into the built-in factory template to create a disabled draft you can review before approval.
                          </p>
                        </div>
                      ) : null}

                      {entry.command_hint ? (
                        <div className="rounded-lg border border-surface2 bg-background/60 px-3 py-2">
                          <p className="text-xs uppercase tracking-[0.2em] text-gray-500">CLI</p>
                          <code className="block pt-1 text-sm text-gray-200 break-all">
                            {entry.command_hint}
                          </code>
                        </div>
                      ) : null}

                      {messages[entry.id] ? (
                        <div className="rounded-lg border border-surface2 bg-background/60 px-3 py-2 text-sm text-gray-300">
                          {messages[entry.id]}
                        </div>
                      ) : null}

                      {previews[entry.id] ? (
                        <PreviewPanel entry={entry} preview={previews[entry.id]} />
                      ) : null}
                    </div>
                  </article>
                ))}
              </div>
            </section>
          ))}
      </main>
    </div>
  );
}

function StatusBadge({ status }: { status: StarterCatalogEntry['status'] }) {
  const className =
    status === 'ready'
      ? 'border-emerald-500/30 bg-emerald-500/10 text-emerald-200'
      : status === 'needs_setup'
        ? 'border-amber-500/30 bg-amber-500/10 text-amber-200'
        : 'border-surface2 bg-background/50 text-gray-300';

  return (
    <span className={`rounded-full border px-2 py-0.5 text-xs ${className}`}>
      {status.replace('_', ' ')}
    </span>
  );
}

function TrustBadge({ official }: { official: boolean }) {
  if (official) {
    return (
      <span className="rounded-full border border-primary/30 bg-primary/10 px-2 py-0.5 text-xs text-primary">
        trusted official
      </span>
    );
  }

  return (
    <span className="rounded-full border border-amber-500/30 bg-amber-500/10 px-2 py-0.5 text-xs text-amber-200">
      review required
    </span>
  );
}

function PreviewPanel({
  entry,
  preview,
}: {
  entry: StarterCatalogEntry;
  preview: AgentFactoryResult | WorkflowFactoryResult;
}) {
  const review = preview.review;
  const tags = preview.spec.tags.slice(0, 6);

  return (
    <div className="space-y-3 rounded-lg border border-primary/20 bg-primary/5 px-3 py-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <p className="text-xs uppercase tracking-[0.2em] text-primary/80">Draft Preview</p>
          <p className="text-sm font-medium text-gray-100">
            {preview.spec.name} · {preview.spec.id}
          </p>
        </div>
        <span className="rounded-full border border-surface2 bg-background/50 px-2 py-0.5 text-xs text-gray-300">
          {review.review_status}
        </span>
      </div>
      <p className="text-sm text-gray-300">{review.summary}</p>
      <div className="grid gap-2 sm:grid-cols-2">
        <div className="rounded-lg border border-surface2 bg-background/60 px-3 py-2 text-sm text-gray-300">
          {entry.kind === 'agent_template'
            ? `${(preview as AgentFactoryResult).spec.capabilities.length} capabilities`
            : `${(preview as WorkflowFactoryResult).spec.steps.length} steps`}
        </div>
        <div className="rounded-lg border border-surface2 bg-background/60 px-3 py-2 text-sm text-gray-300">
          Suggested action: {review.suggested_action}
        </div>
      </div>
      {tags.length > 0 ? (
        <div className="flex flex-wrap gap-2">
          {tags.map((tag) => (
            <span
              key={`${preview.spec.id}:${tag}`}
              className="rounded-full border border-surface2 bg-background/50 px-2 py-0.5 text-xs text-gray-300"
            >
              {tag}
            </span>
          ))}
        </div>
      ) : null}
      {review.warnings.length > 0 ? (
        <ul className="space-y-1 text-sm text-amber-200">
          {review.warnings.slice(0, 3).map((warning) => (
            <li key={`${preview.spec.id}:${warning}`}>- {warning}</li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}

function sectionCopy(kind: StarterCatalogKind): string {
  switch (kind) {
    case 'agent_template':
      return 'Disabled, reviewable AgentSpec starting points for common daemon-native roles.';
    case 'workflow_template':
      return 'Official WorkflowSpec seeds that stay explicit, resumable, and operator-visible.';
    case 'worker_preset':
      return 'Bounded worker profiles for controlled delegation inside workflows and subagents.';
    case 'channel_starter':
      return 'First-class inbound runtime channels that stay under the daemon trust model.';
    case 'capability_pack':
      return 'Curated connector-oriented capability bundles. Trusted one-click install comes next; today these point to the official setup path.';
  }
}

function supportsDraftFactory(kind: StarterCatalogKind) {
  return kind === 'agent_template' || kind === 'workflow_template';
}

function templateId(entry: StarterCatalogEntry) {
  return entry.id.split(':').slice(1).join(':');
}

function defaultRequirement(entry: StarterCatalogEntry) {
  return `Create a ${entry.name} draft for ${entry.description.toLowerCase()}`;
}

function daemonClient() {
  const token = readStoredToken();
  if (!token) {
    throw new Error('Missing daemon token.');
  }
  return new RoveDaemonClient(token);
}

function formatError(error: unknown): string {
  if (error instanceof DaemonError) {
    return error.message;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return 'Unknown error';
}
