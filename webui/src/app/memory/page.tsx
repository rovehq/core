'use client';

import { useEffect, useState } from 'react';

import Nav from '@/components/Nav';
import {
  DaemonError,
  MemoryAdapterMode,
  MemoryBundleStrategy,
  MemoryGraphHit,
  MemoryGraphEnrichment,
  MemoryGraphInspectResponse,
  MemoryMode,
  MemoryQueryResponse,
  MemoryRetrievalAssist,
  MemorySurfaceStatus,
  RoveDaemonClient,
  readStoredToken,
} from '@/lib/daemon';

const DEFAULT_QUERY = 'How is the workflow runtime connected to agent execution?';

export default function MemoryPage() {
  const [surface, setSurface] = useState<MemorySurfaceStatus | null>(null);
  const [inspection, setInspection] = useState<MemoryGraphInspectResponse | null>(null);
  const [queryResponse, setQueryResponse] = useState<MemoryQueryResponse | null>(null);
  const [mode, setMode] = useState<MemoryMode>('graph_only');
  const [bundleStrategy, setBundleStrategy] = useState<MemoryBundleStrategy>('adaptive');
  const [retrievalAssist, setRetrievalAssist] = useState<MemoryRetrievalAssist>('off');
  const [graphEnrichment, setGraphEnrichment] = useState<MemoryGraphEnrichment>('deterministic');
  const [codeAdapterMode, setCodeAdapterMode] = useState<MemoryAdapterMode>('auto');
  const [codeGraphRequired, setCodeGraphRequired] = useState(true);
  const [persistPinnedFacts, setPersistPinnedFacts] = useState(true);
  const [persistTaskTraces, setPersistTaskTraces] = useState(true);
  const [query, setQuery] = useState(DEFAULT_QUERY);
  const [entity, setEntity] = useState('workflow runtime');
  const [note, setNote] = useState('');
  const [domain, setDomain] = useState('code');
  const [backfillBatch, setBackfillBatch] = useState('100');
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    void refresh();
  }, []);

  async function refresh() {
    setLoading(true);
    setError(null);
    try {
      const nextSurface = await daemonClient().getMemorySurface();
      syncSurface(nextSurface);
      const [nextInspection, nextAdapters] = await Promise.all([
        daemonClient().inspectMemoryGraph(entity || null),
        daemonClient().listMemoryAdapters(),
      ]);
      setSurface((current) =>
        current
          ? {
              ...current,
              graph_status: nextAdapters,
            }
          : current,
      );
      setInspection(nextInspection);
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setLoading(false);
    }
  }

  function syncSurface(nextSurface: MemorySurfaceStatus) {
    setSurface(nextSurface);
    setMode(nextSurface.mode);
    setBundleStrategy(nextSurface.bundle_strategy);
    setRetrievalAssist(nextSurface.retrieval_assist);
    setGraphEnrichment(nextSurface.graph_enrichment);
    setCodeAdapterMode(nextSurface.code_adapter_mode);
    setCodeGraphRequired(nextSurface.code_graph_required);
    setPersistPinnedFacts(nextSurface.persist_pinned_facts);
    setPersistTaskTraces(nextSurface.persist_task_traces);
  }

  async function saveSettings() {
    setSaving(true);
    setError(null);
    setMessage(null);
    try {
      const nextSurface = await daemonClient().updateMemorySurface({
        mode,
        bundle_strategy: bundleStrategy,
        retrieval_assist: retrievalAssist,
        graph_enrichment: graphEnrichment,
        code_graph_required: codeGraphRequired,
        code_adapter_mode: codeAdapterMode,
        persist_pinned_facts: persistPinnedFacts,
        persist_task_traces: persistTaskTraces,
      });
      syncSurface(nextSurface);
      setMessage(`Memory settings updated. Mode is ${mode}.`);
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setSaving(false);
    }
  }

  async function reindex() {
    setSaving(true);
    setError(null);
    setMessage(null);
    try {
      const nextSurface = await daemonClient().reindexMemory();
      syncSurface(nextSurface);
      const [nextInspection, nextAdapters] = await Promise.all([
        daemonClient().inspectMemoryGraph(entity || null),
        daemonClient().refreshMemoryAdapters(),
      ]);
      setSurface((current) =>
        current
          ? {
              ...current,
              graph_status: nextAdapters,
            }
          : current,
      );
      setInspection(nextInspection);
      setMessage('Reindexed the structural code adapter sources.');
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setSaving(false);
    }
  }

  async function refreshAdapters() {
    setSaving(true);
    setError(null);
    setMessage(null);
    try {
      const nextAdapters = await daemonClient().refreshMemoryAdapters();
      setSurface((current) =>
        current
          ? {
              ...current,
              graph_status: nextAdapters,
            }
          : current,
      );
      setMessage('Refreshed structural adapter status.');
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setSaving(false);
    }
  }

  async function backfill() {
    setSaving(true);
    setError(null);
    setMessage(null);
    try {
      const batch_size = Math.max(1, Number.parseInt(backfillBatch, 10) || 100);
      const result = await daemonClient().backfillMemory({ batch_size });
      syncSurface(result.status);
      setMessage(`Backfilled embeddings for ${result.backfilled} episodic record(s).`);
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setSaving(false);
    }
  }

  async function runQuery() {
    setSaving(true);
    setError(null);
    setMessage(null);
    try {
      setQueryResponse(
        await daemonClient().queryMemory({
          question: query,
          explain: true,
          domain,
        }),
      );
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setSaving(false);
    }
  }

  async function inspectEntity() {
    setSaving(true);
    setError(null);
    setMessage(null);
    try {
      setInspection(await daemonClient().inspectMemoryGraph(entity || null));
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setSaving(false);
    }
  }

  async function ingestNote() {
    if (!note.trim()) {
      return;
    }
    setSaving(true);
    setError(null);
    setMessage(null);
    try {
      const hit = await daemonClient().ingestMemoryNote({ note, domain });
      setMessage(`Stored note: ${hit.content}`);
      setNote('');
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="min-h-screen flex flex-col">
      <header className="sticky top-0 z-10 bg-background/80 backdrop-blur border-b border-surface2">
        <div className="max-w-6xl mx-auto px-4 py-4 space-y-4">
          <div>
            <h1 className="text-2xl font-semibold">Memory Control Plane</h1>
            <p className="text-sm text-gray-400">
              Graph-first by default. Always-on memory is explicit and opt-in.
            </p>
          </div>
          <Nav />
        </div>
      </header>

      <main className="flex-1 max-w-6xl w-full mx-auto px-4 py-6 space-y-6">
        <section className="grid gap-4 md:grid-cols-3 xl:grid-cols-6">
          <StatCard label="Mode" value={surface?.mode ?? 'loading'} />
          <StatCard
            label="Graph Health"
            value={surface?.graph_status.healthy ? 'healthy' : 'degraded'}
          />
          <StatCard
            label="Imported Repos"
            value={`${surface?.graph_status.imported_count ?? 0}/${surface?.graph_status.available_count ?? 0}`}
          />
          <StatCard
            label="Graph Size"
            value={`${surface?.graph_stats.nodes ?? 0} nodes`}
            detail={`${surface?.graph_stats.edges ?? 0} edges`}
          />
          <StatCard
            label="Pinned Facts"
            value={`${surface?.memory_stats.facts ?? 0}`}
          />
          <StatCard
            label="Task Traces"
            value={`${surface?.memory_stats.task_traces ?? 0}`}
            detail={`${surface?.memory_stats.memory_graph_edges ?? 0} memory edges`}
          />
          <StatCard
            label="Embeddings"
            value={`${surface?.memory_stats.embedded_episodic ?? 0}/${surface?.memory_stats.total_episodic ?? 0}`}
            detail={`${surface?.memory_stats.embedding_coverage_pct?.toFixed(1) ?? '0.0'}% covered`}
          />
        </section>

        {error ? (
          <section className="rounded-xl border border-red-500/40 bg-red-500/10 px-4 py-3 text-sm text-red-200">
            {error}
          </section>
        ) : null}

        {message ? (
          <section className="rounded-xl border border-emerald-500/40 bg-emerald-500/10 px-4 py-3 text-sm text-emerald-200">
            {message}
          </section>
        ) : null}

        <section className="grid gap-6 lg:grid-cols-[1.1fr_0.9fr]">
          <Panel title="Mode">
            <div className="space-y-4">
              <label className="block space-y-2 text-sm text-gray-300">
                <span>Memory mode</span>
                <select
                  value={mode}
                  onChange={(event) => setMode(event.target.value as MemoryMode)}
                  className="w-full rounded-lg border border-surface2 bg-surface px-3 py-2"
                >
                  <option value="graph_only">graph_only</option>
                  <option value="always_on">always_on</option>
                </select>
              </label>
              <label className="block space-y-2 text-sm text-gray-300">
                <span>Code adapter mode</span>
                <select
                  value={codeAdapterMode}
                  onChange={(event) => setCodeAdapterMode(event.target.value as MemoryAdapterMode)}
                  className="w-full rounded-lg border border-surface2 bg-surface px-3 py-2"
                >
                  <option value="off">off</option>
                  <option value="auto">auto</option>
                  <option value="required">required</option>
                </select>
              </label>
              <label className="block space-y-2 text-sm text-gray-300">
                <span>Retrieval assist</span>
                <select
                  value={retrievalAssist}
                  onChange={(event) =>
                    setRetrievalAssist(event.target.value as MemoryRetrievalAssist)
                  }
                  className="w-full rounded-lg border border-surface2 bg-surface px-3 py-2"
                >
                  <option value="off">off</option>
                  <option value="rerank">rerank</option>
                  <option value="compress">compress</option>
                </select>
              </label>
              <label className="block space-y-2 text-sm text-gray-300">
                <span>Graph enrichment</span>
                <select
                  value={graphEnrichment}
                  onChange={(event) =>
                    setGraphEnrichment(event.target.value as MemoryGraphEnrichment)
                  }
                  className="w-full rounded-lg border border-surface2 bg-surface px-3 py-2"
                >
                  <option value="deterministic">deterministic</option>
                  <option value="deterministic_plus_llm">deterministic_plus_llm</option>
                </select>
              </label>
              <div className="grid gap-2 text-sm text-gray-300 sm:grid-cols-2">
                <ToggleRow
                  label="Require code adapter"
                  checked={codeGraphRequired}
                  onChange={setCodeGraphRequired}
                />
                <ToggleRow
                  label="Persist pinned facts"
                  checked={persistPinnedFacts}
                  onChange={setPersistPinnedFacts}
                />
                <ToggleRow
                  label="Persist task traces"
                  checked={persistTaskTraces}
                  onChange={setPersistTaskTraces}
                />
                <div className="rounded-lg border border-surface2/80 bg-surface/60 px-3 py-2">
                  <div className="text-xs uppercase tracking-[0.18em] text-gray-500">
                    Bundle
                  </div>
                  <div className="mt-1 text-gray-200">{bundleStrategy}</div>
                </div>
              </div>
              <div className="flex flex-wrap gap-3">
                <button
                  onClick={saveSettings}
                  disabled={saving || loading}
                  className="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-white disabled:opacity-50"
                >
                  Save settings
                </button>
                <button
                  onClick={reindex}
                  disabled={saving || loading}
                  className="rounded-lg border border-surface2 px-4 py-2 text-sm text-gray-200 disabled:opacity-50"
                >
                  Reindex graph
                </button>
              </div>
              <ul className="space-y-2 text-sm text-gray-400">
                {surface?.warnings.map((warning) => (
                  <li key={warning} className="rounded-lg border border-surface2/80 bg-surface/80 px-3 py-2">
                    {warning}
                  </li>
                ))}
              </ul>
            </div>
          </Panel>

          <Panel title="Graph Sources">
            <div className="space-y-3">
              <div className="flex flex-wrap gap-3">
                <button
                  onClick={refreshAdapters}
                  disabled={saving || loading}
                  className="rounded-lg border border-surface2 px-4 py-2 text-sm text-gray-200 disabled:opacity-50"
                >
                  Refresh adapters
                </button>
                <div className="flex items-center gap-2">
                  <input
                    value={backfillBatch}
                    onChange={(event) => setBackfillBatch(event.target.value)}
                    className="w-24 rounded-lg border border-surface2 bg-surface px-3 py-2 text-sm"
                  />
                  <button
                    onClick={backfill}
                    disabled={saving || loading}
                    className="rounded-lg border border-surface2 px-4 py-2 text-sm text-gray-200 disabled:opacity-50"
                  >
                    Backfill embeddings
                  </button>
                </div>
              </div>
              {surface?.graph_status.repos.map((repo) => (
                <div key={repo.repo_name} className="rounded-lg border border-surface2/80 bg-surface/70 px-3 py-3 text-sm">
                  <div className="flex items-center justify-between gap-3">
                    <strong>{repo.repo_name}</strong>
                    <span className={repo.stale ? 'text-amber-300' : 'text-emerald-300'}>
                      {repo.stale ? 'stale' : 'current'}
                    </span>
                  </div>
                  <div className="mt-2 text-gray-400">
                    nodes {repo.nodes} · edges {repo.edges} · files {repo.files}
                  </div>
                  {repo.message ? <div className="mt-2 text-amber-200">{repo.message}</div> : null}
                </div>
              ))}
            </div>
          </Panel>
        </section>

        <section className="grid gap-6 lg:grid-cols-[1fr_1fr]">
          <Panel title="Query Inspector">
            <div className="space-y-4">
              <label className="block space-y-2 text-sm text-gray-300">
                <span>Question</span>
                <textarea
                  value={query}
                  onChange={(event) => setQuery(event.target.value)}
                  rows={4}
                  className="w-full rounded-lg border border-surface2 bg-surface px-3 py-2"
                />
              </label>
              <label className="block space-y-2 text-sm text-gray-300">
                <span>Domain</span>
                <input
                  value={domain}
                  onChange={(event) => setDomain(event.target.value)}
                  className="w-full rounded-lg border border-surface2 bg-surface px-3 py-2"
                />
              </label>
              <button
                onClick={runQuery}
                disabled={saving || loading}
                className="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-white disabled:opacity-50"
              >
                Run query
              </button>

              {queryResponse?.explain ? (
                <div className="rounded-lg border border-surface2/80 bg-surface/80 px-3 py-3 text-sm text-gray-300">
                  <div>intent: {queryResponse.explain.intent}</div>
                  <div>mode: {queryResponse.explain.mode}</div>
                  <div>bundle: {surface?.bundle_strategy ?? 'adaptive'}</div>
                  <div>sources: {queryResponse.explain.sources.join(', ')}</div>
                  <div>graph paths: {queryResponse.explain.graph_paths_used}</div>
                  <div>memory graph: {queryResponse.explain.memory_graph_hits_used}</div>
                  <div>task traces: {queryResponse.explain.task_trace_hits_used}</div>
                  {queryResponse.explain.fallback_reason ? (
                    <div>fallback: {queryResponse.explain.fallback_reason}</div>
                  ) : null}
                </div>
              ) : null}
            </div>
          </Panel>

          <Panel title="Graph Inspect">
            <div className="space-y-4">
              <label className="block space-y-2 text-sm text-gray-300">
                <span>Entity</span>
                <input
                  value={entity}
                  onChange={(event) => setEntity(event.target.value)}
                  className="w-full rounded-lg border border-surface2 bg-surface px-3 py-2"
                />
              </label>
              <button
                onClick={inspectEntity}
                disabled={saving || loading}
                className="rounded-lg border border-surface2 px-4 py-2 text-sm text-gray-200 disabled:opacity-50"
              >
                Inspect graph
              </button>

              <div className="space-y-2">
                {inspection?.paths.map((path) => (
                  <div key={`${path.summary}-${path.score}`} className="rounded-lg border border-surface2/80 bg-surface/70 px-3 py-3 text-sm">
                    <div className="text-gray-100">{path.summary}</div>
                    <div className="mt-1 text-gray-400">
                      {path.source_kinds.join(', ')} · confidence {path.confidence.toFixed(2)}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          </Panel>
        </section>

        <section className="grid gap-6 lg:grid-cols-[1fr_1fr]">
          <Panel title="Structured Context">
            <HitGroup label="Facts" hits={queryResponse?.facts ?? []} />
            <HitGroup label="Preferences" hits={queryResponse?.preferences ?? []} />
            <HitGroup label="Warnings" hits={queryResponse?.warnings ?? []} />
            <HitGroup label="Errors" hits={queryResponse?.errors ?? []} />
            <HitGroup label="Insights" hits={queryResponse?.insight_hits ?? []} />
            <HitGroup label="Episodes" hits={queryResponse?.episodic_hits ?? []} />
            <HitGroup label="Task Traces" hits={queryResponse?.task_trace_hits ?? []} />
            <MemoryGraphHitGroup hits={queryResponse?.memory_graph_hits ?? []} />
          </Panel>

          <Panel title="Manual Note">
            <div className="space-y-4">
              <textarea
                value={note}
                onChange={(event) => setNote(event.target.value)}
                rows={5}
                placeholder="Remember that the workflow runtime rollout needs operator review..."
                className="w-full rounded-lg border border-surface2 bg-surface px-3 py-2"
              />
              <button
                onClick={ingestNote}
                disabled={saving || loading || !note.trim()}
                className="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-white disabled:opacity-50"
              >
                Ingest note
              </button>
            </div>
          </Panel>
        </section>
      </main>
    </div>
  );
}

function Panel({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="rounded-2xl border border-surface2 bg-surface/80 p-5 shadow-[0_16px_40px_rgba(0,0,0,0.22)]">
      <h2 className="text-lg font-semibold text-gray-100">{title}</h2>
      <div className="mt-4">{children}</div>
    </section>
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
    <div className="rounded-2xl border border-surface2 bg-surface/80 px-4 py-4 shadow-[0_16px_40px_rgba(0,0,0,0.18)]">
      <div className="text-xs uppercase tracking-[0.24em] text-gray-500">{label}</div>
      <div className="mt-2 text-xl font-semibold text-gray-100">{value}</div>
      {detail ? <div className="mt-1 text-sm text-gray-400">{detail}</div> : null}
    </div>
  );
}

function HitGroup({ label, hits }: { label: string; hits: { id: string; source: string; content: string }[] }) {
  if (!hits.length) {
    return null;
  }
  return (
    <div className="mb-4">
      <h3 className="mb-2 text-sm font-medium text-gray-300">{label}</h3>
      <div className="space-y-2">
        {hits.map((hit) => (
          <div key={hit.id} className="rounded-lg border border-surface2/80 bg-background/40 px-3 py-3 text-sm">
            <div className="text-gray-100">{hit.content}</div>
            <div className="mt-1 text-xs uppercase tracking-[0.18em] text-gray-500">{hit.source}</div>
          </div>
        ))}
      </div>
    </div>
  );
}

function MemoryGraphHitGroup({ hits }: { hits: MemoryGraphHit[] }) {
  if (!hits.length) {
    return null;
  }
  return (
    <div className="mb-4">
      <h3 className="mb-2 text-sm font-medium text-gray-300">Memory Graph Paths</h3>
      <div className="space-y-2">
        {hits.map((hit) => (
          <div
            key={`${hit.id}-${hit.depth}`}
            className="rounded-lg border border-indigo-500/30 bg-indigo-950/30 px-3 py-3 text-sm"
          >
            <div className="text-gray-100">{hit.content}</div>
            <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-gray-400">
              <span className="rounded bg-indigo-800/50 px-1.5 py-0.5 text-indigo-200">
                depth {hit.depth}
              </span>
              <span>score {hit.graph_score.toFixed(3)}</span>
              <span className="capitalize">{hit.memory_kind}</span>
            </div>
            {hit.path_edge_types.length > 0 ? (
              <div className="mt-1.5 text-xs text-gray-500">
                via {hit.path_edge_types.join(' → ')}
              </div>
            ) : null}
          </div>
        ))}
      </div>
    </div>
  );
}

function ToggleRow({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <label className="flex items-center justify-between gap-3 rounded-lg border border-surface2/80 bg-surface/60 px-3 py-2">
      <span>{label}</span>
      <input
        type="checkbox"
        checked={checked}
        onChange={(event) => onChange(event.target.checked)}
        className="h-4 w-4 rounded border-surface2 bg-background text-primary"
      />
    </label>
  );
}

function daemonClient() {
  return new RoveDaemonClient(readStoredToken() ?? undefined);
}

function formatError(error: unknown): string {
  if (error instanceof DaemonError) {
    return error.message;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return 'Unknown daemon error';
}
