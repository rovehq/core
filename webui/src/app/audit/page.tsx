'use client';

import { useEffect, useState } from 'react';

import { RoveDaemonClient, type AuditLogRecord } from '@/lib/daemon';

type SeverityFilter = '' | 'low' | 'medium' | 'high';

export default function AuditPage() {
  const [records, setRecords] = useState<AuditLogRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [source, setSource] = useState('');
  const [action, setAction] = useState('');
  const [severity, setSeverity] = useState<SeverityFilter>('');

  useEffect(() => {
    void refresh();
  }, [source, action, severity]);

  async function refresh() {
    setLoading(true);
    setError(null);
    try {
      const client = new RoveDaemonClient();
      const next = await client.listAuditLog({
        source: source || undefined,
        action: action || undefined,
        severity: severity || undefined,
        limit: 200,
      });
      setRecords(next);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load audit log');
    } finally {
      setLoading(false);
    }
  }

  return (
    <main className="space-y-6">
      <section className="rounded-3xl border border-surface2/80 bg-surface/80 p-6 shadow-[0_18px_40px_rgba(0,0,0,0.22)] backdrop-blur">
        <div className="flex flex-col gap-4 lg:flex-row lg:items-end lg:justify-between">
          <div>
            <h1 className="text-2xl font-semibold text-white">Audit Log</h1>
            <p className="mt-2 max-w-2xl text-sm text-gray-400">
              Query tool execution audit records by action, source, and severity.
            </p>
          </div>
          <div className="grid gap-3 sm:grid-cols-3">
            <label className="space-y-1 text-xs uppercase tracking-[0.24em] text-gray-500">
              <span>Source</span>
              <input
                value={source}
                onChange={(event) => setSource(event.target.value)}
                placeholder="cli, webui, telegram:123"
                className="w-full rounded-xl border border-surface2 bg-background/70 px-3 py-2 text-sm normal-case tracking-normal text-white outline-none"
              />
            </label>
            <label className="space-y-1 text-xs uppercase tracking-[0.24em] text-gray-500">
              <span>Action</span>
              <input
                value={action}
                onChange={(event) => setAction(event.target.value)}
                placeholder="tool_execution"
                className="w-full rounded-xl border border-surface2 bg-background/70 px-3 py-2 text-sm normal-case tracking-normal text-white outline-none"
              />
            </label>
            <label className="space-y-1 text-xs uppercase tracking-[0.24em] text-gray-500">
              <span>Severity</span>
              <select
                value={severity}
                onChange={(event) => setSeverity(event.target.value as SeverityFilter)}
                className="w-full rounded-xl border border-surface2 bg-background/70 px-3 py-2 text-sm normal-case tracking-normal text-white outline-none"
              >
                <option value="">All</option>
                <option value="low">Low</option>
                <option value="medium">Medium</option>
                <option value="high">High</option>
              </select>
            </label>
          </div>
        </div>
      </section>

      <section className="rounded-3xl border border-surface2/80 bg-surface/80 p-6 shadow-[0_18px_40px_rgba(0,0,0,0.22)] backdrop-blur">
        {loading ? (
          <p className="text-sm text-gray-400">Loading audit log…</p>
        ) : error ? (
          <p className="text-sm text-red-300">{error}</p>
        ) : records.length === 0 ? (
          <p className="text-sm text-gray-400">No audit log entries matched the current filters.</p>
        ) : (
          <div className="space-y-3">
            {records.map((record) => (
              <article
                key={record.id}
                className="rounded-2xl border border-surface2/70 bg-background/50 p-4"
              >
                <div className="flex flex-col gap-2 lg:flex-row lg:items-center lg:justify-between">
                  <div className="flex flex-wrap items-center gap-2 text-sm text-white">
                    <span className="font-medium">{record.tool_name}</span>
                    <span className="rounded-full border border-surface2 px-2 py-0.5 text-xs text-gray-300">
                      {record.action_type}
                    </span>
                    <span
                      className={`rounded-full px-2 py-0.5 text-xs ${
                        record.severity === 'high'
                          ? 'bg-red-500/15 text-red-200'
                          : record.severity === 'medium'
                            ? 'bg-amber-500/15 text-amber-200'
                            : 'bg-emerald-500/15 text-emerald-200'
                      }`}
                    >
                      {record.severity}
                    </span>
                  </div>
                  <div className="text-xs text-gray-400">
                    {new Date(record.timestamp * 1000).toLocaleString()}
                  </div>
                </div>
                <div className="mt-2 grid gap-1 text-xs text-gray-400 sm:grid-cols-2 lg:grid-cols-4">
                  <span>source: {record.source ?? 'unknown'}</span>
                  <span>approved_by: {record.approved_by}</span>
                  <span>task: {record.task_id}</span>
                  <span>risk_tier: {record.risk_tier}</span>
                </div>
                <p className="mt-3 text-sm text-gray-200">{record.result_summary}</p>
              </article>
            ))}
          </div>
        )}
      </section>
    </main>
  );
}
