'use client';

import { useEffect, useState } from 'react';

import { RoveDaemonClient, type MigrationImportResult, type MigrationReport, type MigrationStatusReport } from '@/lib/daemon';

type Source = 'openclaw' | 'zeroclaw' | 'moltis';

export default function MigratePage() {
  const [source, setSource] = useState<Source>('openclaw');
  const [path, setPath] = useState('');
  const [inspect, setInspect] = useState<MigrationReport | null>(null);
  const [status, setStatus] = useState<MigrationStatusReport | null>(null);
  const [lastImport, setLastImport] = useState<MigrationImportResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void refreshStatus();
  }, []);

  async function refreshStatus() {
    setLoading(true);
    setError(null);
    try {
      const client = new RoveDaemonClient();
      setStatus(await client.migrationStatus());
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load migration status');
    } finally {
      setLoading(false);
    }
  }

  async function runInspect() {
    setBusy(true);
    setError(null);
    try {
      const client = new RoveDaemonClient();
      setInspect(await client.inspectMigration(source, path || undefined));
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to inspect migration source');
    } finally {
      setBusy(false);
    }
  }

  async function runImport() {
    setBusy(true);
    setError(null);
    try {
      const client = new RoveDaemonClient();
      const result = await client.importMigration(source, path || undefined);
      setLastImport(result);
      await refreshStatus();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to import migration source');
    } finally {
      setBusy(false);
    }
  }

  return (
    <main className="space-y-6">
      <section className="rounded-3xl border border-surface2/80 bg-surface/80 p-6 shadow-[0_18px_40px_rgba(0,0,0,0.22)] backdrop-blur">
        <h1 className="text-2xl font-semibold text-white">Migration</h1>
        <p className="mt-2 max-w-2xl text-sm text-gray-400">
          Inspect and import compatible agents and workflows from supported source installs.
        </p>
        <div className="mt-5 grid gap-3 lg:grid-cols-[180px_minmax(0,1fr)_auto_auto]">
          <select
            value={source}
            onChange={(event) => setSource(event.target.value as Source)}
            className="rounded-xl border border-surface2 bg-background/70 px-3 py-2 text-sm text-white outline-none"
          >
            <option value="openclaw">OpenClaw</option>
            <option value="zeroclaw">ZeroClaw</option>
            <option value="moltis">Moltis</option>
          </select>
          <input
            value={path}
            onChange={(event) => setPath(event.target.value)}
            placeholder="Optional source root override"
            className="rounded-xl border border-surface2 bg-background/70 px-3 py-2 text-sm text-white outline-none"
          />
          <button
            onClick={runInspect}
            disabled={busy}
            className="rounded-xl border border-surface2 px-4 py-2 text-sm text-white transition hover:border-primary hover:bg-primary/10 disabled:opacity-60"
          >
            Inspect
          </button>
          <button
            onClick={runImport}
            disabled={busy}
            className="rounded-xl border border-primary/70 bg-primary/90 px-4 py-2 text-sm text-white shadow-[0_14px_28px_rgba(222,105,71,0.24)] disabled:opacity-60"
          >
            Import
          </button>
        </div>
        {error ? <p className="mt-4 text-sm text-red-300">{error}</p> : null}
      </section>

      {inspect ? (
        <section className="rounded-3xl border border-surface2/80 bg-surface/80 p-6 shadow-[0_18px_40px_rgba(0,0,0,0.22)] backdrop-blur">
          <h2 className="text-lg font-semibold text-white">Inspection</h2>
          <p className="mt-2 text-sm text-gray-400">
            root: <code>{inspect.root}</code>
          </p>
          <div className="mt-4 grid gap-3 sm:grid-cols-3">
            <Metric label="Agents" value={inspect.agent_candidates.length} />
            <Metric label="Workflows" value={inspect.workflow_candidates.length} />
            <Metric label="Channels" value={inspect.detected_channels.length} />
          </div>
          {inspect.warnings.length > 0 ? (
            <div className="mt-4 space-y-1 text-sm text-amber-200">
              {inspect.warnings.map((warning) => <p key={warning}>{warning}</p>)}
            </div>
          ) : null}
        </section>
      ) : null}

      {lastImport ? (
        <section className="rounded-3xl border border-surface2/80 bg-surface/80 p-6 shadow-[0_18px_40px_rgba(0,0,0,0.22)] backdrop-blur">
          <h2 className="text-lg font-semibold text-white">Last Import</h2>
          <div className="mt-4 grid gap-3 sm:grid-cols-2">
            <Metric label="Imported agents" value={lastImport.imported_agents.length} />
            <Metric label="Imported workflows" value={lastImport.imported_workflows.length} />
          </div>
          {lastImport.warnings.length > 0 ? (
            <div className="mt-4 space-y-1 text-sm text-amber-200">
              {lastImport.warnings.map((warning) => <p key={warning}>{warning}</p>)}
            </div>
          ) : null}
        </section>
      ) : null}

      <section className="rounded-3xl border border-surface2/80 bg-surface/80 p-6 shadow-[0_18px_40px_rgba(0,0,0,0.22)] backdrop-blur">
        <h2 className="text-lg font-semibold text-white">Imported Specs</h2>
        {loading ? (
          <p className="mt-3 text-sm text-gray-400">Loading migration status…</p>
        ) : !status || status.per_source.length === 0 ? (
          <p className="mt-3 text-sm text-gray-400">No imported specs recorded yet.</p>
        ) : (
          <div className="mt-4 space-y-4">
            {status.per_source.map((entry) => (
              <article key={entry.source} className="rounded-2xl border border-surface2/70 bg-background/50 p-4">
                <h3 className="text-sm font-semibold uppercase tracking-[0.24em] text-gray-400">{entry.source}</h3>
                <div className="mt-3 grid gap-3 sm:grid-cols-2">
                  <Metric label="Agents" value={entry.agents.length} />
                  <Metric label="Workflows" value={entry.workflows.length} />
                </div>
              </article>
            ))}
          </div>
        )}
      </section>
    </main>
  );
}

function Metric({ label, value }: { label: string; value: number }) {
  return (
    <div className="rounded-2xl border border-surface2/70 bg-background/50 p-4">
      <div className="text-xs uppercase tracking-[0.24em] text-gray-500">{label}</div>
      <div className="mt-2 text-2xl font-semibold text-white">{value}</div>
    </div>
  );
}
