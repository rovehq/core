'use client';

import { useCallback, useEffect, useRef, useState } from 'react';

import Nav from '@/components/Nav';
import {
  DaemonError,
  KnowledgeDocument,
  KnowledgeIngestResult,
  KnowledgeIngestSummary,
  KnowledgeJob,
  KnowledgeSearchHit,
  KnowledgeStats,
  RoveDaemonClient,
  readStoredToken,
} from '@/lib/daemon';

type IngestTab = 'upload' | 'file' | 'folder' | 'url' | 'sitemap';

const SUPPORTED_EXTENSIONS =
  'md txt rst json toml yaml yml csv html htm xml log py rs ts js go java c cpp h hpp sh bash zsh sql';

function daemonClient() {
  return new RoveDaemonClient(readStoredToken() ?? undefined);
}

function fmtTimestamp(ts: number): string {
  return new Date(ts * 1000).toLocaleString();
}

function fmtWords(n: number | null): string {
  if (n == null) return '—';
  if (n >= 1000) return `${(n / 1000).toFixed(1)}k`;
  return String(n);
}

export default function KnowledgePage() {
  const [docs, setDocs] = useState<KnowledgeDocument[]>([]);
  const [stats, setStats] = useState<KnowledgeStats | null>(null);
  const [selected, setSelected] = useState<KnowledgeDocument | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  // Search
  const [searchQuery, setSearchQuery] = useState('');
  const [searchResults, setSearchResults] = useState<KnowledgeSearchHit[] | null>(null);
  const [searching, setSearching] = useState(false);
  const searchTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Ingest form
  const [ingestTab, setIngestTab] = useState<IngestTab>('upload');
  const [ingestPath, setIngestPath] = useState('');
  const [ingestUrl, setIngestUrl] = useState('');
  const [ingestDomain, setIngestDomain] = useState('');
  const [ingestTags, setIngestTags] = useState('');
  const [ingestForce, setIngestForce] = useState(false);
  const [ingestDryRun, setIngestDryRun] = useState(false);
  const [ingesting, setIngesting] = useState(false);
  const [ingestResult, setIngestResult] = useState<
    KnowledgeIngestResult | KnowledgeIngestSummary | null
  >(null);

  // Background jobs
  const [activeJob, setActiveJob] = useState<KnowledgeJob | null>(null);
  const jobPollTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Upload / drag-and-drop
  const [dragOver, setDragOver] = useState(false);
  const [uploadFiles, setUploadFiles] = useState<File[]>([]);
  const fileInputRef = useRef<HTMLInputElement>(null);

  // Deletion
  const [deleting, setDeleting] = useState<string | null>(null);

  useEffect(() => {
    void refresh();
    return () => {
      if (jobPollTimer.current) clearTimeout(jobPollTimer.current);
    };
  }, []);

  async function refresh() {
    setLoading(true);
    setError(null);
    try {
      const client = daemonClient();
      const [docList, statsData] = await Promise.all([
        client.listKnowledge({ limit: 100 }),
        client.knowledgeStats(),
      ]);
      setDocs(docList);
      setStats(statsData);
    } catch (e) {
      setError(e instanceof DaemonError ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }

  function pollJob(jobId: string) {
    if (jobPollTimer.current) clearTimeout(jobPollTimer.current);
    jobPollTimer.current = setTimeout(async () => {
      try {
        const job = await daemonClient().getKnowledgeJob(jobId);
        setActiveJob(job);
        if (job.status === 'running') {
          pollJob(jobId);
        } else {
          void refresh();
        }
      } catch {
        // ignore poll errors
      }
    }, 1500);
  }

  function handleSearchChange(value: string) {
    setSearchQuery(value);
    if (searchTimer.current) clearTimeout(searchTimer.current);
    if (!value.trim()) {
      setSearchResults(null);
      return;
    }
    searchTimer.current = setTimeout(async () => {
      setSearching(true);
      try {
        const results = await daemonClient().searchKnowledge(value.trim(), 20);
        setSearchResults(results);
      } catch {
        // ignore transient search errors
      } finally {
        setSearching(false);
      }
    }, 350);
  }

  async function handleDelete(doc: KnowledgeDocument) {
    if (!confirm(`Delete "${doc.title ?? doc.source_path}"?`)) return;
    setDeleting(doc.id);
    try {
      await daemonClient().removeKnowledge(doc.id);
      if (selected?.id === doc.id) setSelected(null);
      setMessage(`Deleted: ${doc.title ?? doc.source_path}`);
      void refresh();
    } catch (e) {
      setError(e instanceof DaemonError ? e.message : String(e));
    } finally {
      setDeleting(null);
    }
  }

  const onDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setDragOver(true);
  }, []);

  const onDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setDragOver(false);
  }, []);

  const onDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setDragOver(false);
    const dropped = Array.from(e.dataTransfer.files);
    if (dropped.length) {
      setUploadFiles((prev) => {
        const names = new Set(prev.map((f) => f.name));
        return [...prev, ...dropped.filter((f) => !names.has(f.name))];
      });
    }
  }, []);

  function onFileInput(e: React.ChangeEvent<HTMLInputElement>) {
    const picked = Array.from(e.target.files ?? []);
    if (picked.length) {
      setUploadFiles((prev) => {
        const names = new Set(prev.map((f) => f.name));
        return [...prev, ...picked.filter((f) => !names.has(f.name))];
      });
    }
    if (fileInputRef.current) fileInputRef.current.value = '';
  }

  async function handleUpload() {
    if (!uploadFiles.length) return;
    setIngesting(true);
    setIngestResult(null);
    setError(null);
    try {
      const summary = await daemonClient().uploadKnowledgeFiles(uploadFiles);
      setIngestResult(summary);
      setUploadFiles([]);
      void refresh();
    } catch (e) {
      setError(e instanceof DaemonError ? e.message : String(e));
    } finally {
      setIngesting(false);
    }
  }

  async function handleIngest() {
    setIngesting(true);
    setIngestResult(null);
    setActiveJob(null);
    setError(null);
    const client = daemonClient();
    const domain = ingestDomain.trim() || undefined;
    const tags = ingestTags.split(',').map((t) => t.trim()).filter(Boolean);

    try {
      if (ingestTab === 'file') {
        const result = await client.ingestKnowledgeFile({
          path: ingestPath.trim(), domain,
          tags: tags.length ? tags : undefined,
          force: ingestForce || undefined,
        });
        setIngestResult(result);
        void refresh();
      } else if (ingestTab === 'folder') {
        const resp = await client.ingestKnowledgeFolder({
          path: ingestPath.trim(), domain,
          tags: tags.length ? tags : undefined,
          force: ingestForce || undefined,
          dry_run: ingestDryRun || undefined,
        });
        if ('job_id' in (resp as object)) {
          const jobId = (resp as unknown as { job_id: string }).job_id;
          const job = await client.getKnowledgeJob(jobId);
          setActiveJob(job);
          pollJob(jobId);
        } else {
          setIngestResult(resp as KnowledgeIngestSummary);
          void refresh();
        }
      } else if (ingestTab === 'url') {
        const result = await client.ingestKnowledgeUrl({
          url: ingestUrl.trim(), domain,
          tags: tags.length ? tags : undefined,
          force: ingestForce || undefined,
        });
        setIngestResult(result);
        void refresh();
      } else {
        const resp = await client.ingestKnowledgeSitemap({
          url: ingestUrl.trim(), domain,
          tags: tags.length ? tags : undefined,
          force: ingestForce || undefined,
          dry_run: ingestDryRun || undefined,
        });
        if ('job_id' in (resp as object)) {
          const jobId = (resp as unknown as { job_id: string }).job_id;
          const job = await client.getKnowledgeJob(jobId);
          setActiveJob(job);
          pollJob(jobId);
        } else {
          setIngestResult(resp as KnowledgeIngestSummary);
          void refresh();
        }
      }
    } catch (e) {
      setError(e instanceof DaemonError ? e.message : String(e));
    } finally {
      setIngesting(false);
    }
  }

  // Rows shown in the table — search hits show the doc, otherwise full list
  const displayDocs: KnowledgeDocument[] = searchResults
    ? searchResults.map((h) => h.doc)
    : docs;

  return (
    <main className="flex min-h-screen flex-col items-start gap-6 p-6">
      <Nav />

      <div className="w-full max-w-6xl">
        <h1 className="mb-1 text-2xl font-bold text-white">Knowledge</h1>
        <p className="text-sm text-gray-400">
          Ingest and manage documents available to agents as retrieval context.
        </p>
      </div>

      {/* Stats strip */}
      {stats && (
        <div className="flex flex-wrap gap-4">
          <StatBadge label="Documents" value={String(stats.total_documents)} />
          <StatBadge label="Words" value={fmtWords(stats.total_words)} />
          {stats.by_source.map((s) => (
            <StatBadge key={s.source_type} label={s.source_type} value={`${s.count} docs`} />
          ))}
        </div>
      )}

      {error && (
        <div className="w-full rounded-lg border border-red-500/40 bg-red-500/10 px-4 py-2 text-sm text-red-400">
          {error}
        </div>
      )}
      {message && (
        <div className="w-full rounded-lg border border-green-500/40 bg-green-500/10 px-4 py-2 text-sm text-green-400">
          {message}
        </div>
      )}

      <div className="flex w-full max-w-6xl flex-col gap-6 lg:flex-row">
        {/* Left: document list */}
        <div className="flex-1 min-w-0">
          <div className="mb-3 flex items-center gap-2">
            <input
              type="text"
              placeholder="Search documents…"
              value={searchQuery}
              onChange={(e) => handleSearchChange(e.target.value)}
              className="w-full rounded-lg border border-surface2 bg-background px-3 py-2 text-sm text-white placeholder-gray-500 focus:outline-none focus:ring-1 focus:ring-primary"
            />
            {searching && <span className="text-xs text-gray-500">…</span>}
            <button
              onClick={() => void refresh()}
              className="rounded-lg border border-surface2 bg-surface px-3 py-2 text-xs text-gray-400 hover:text-white"
            >
              Refresh
            </button>
          </div>

          {loading ? (
            <p className="text-sm text-gray-500">Loading…</p>
          ) : displayDocs.length === 0 ? (
            <p className="text-sm text-gray-500">
              {searchResults != null ? 'No results.' : 'No documents ingested yet.'}
            </p>
          ) : (
            <div className="overflow-x-auto rounded-xl border border-surface2/60">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-surface2/60 text-left text-xs text-gray-500">
                    <th className="px-3 py-2">Title</th>
                    <th className="px-3 py-2">Source</th>
                    <th className="px-3 py-2">Domain</th>
                    <th className="px-3 py-2">Words</th>
                    <th className="px-3 py-2">Indexed</th>
                    <th className="px-3 py-2"></th>
                  </tr>
                </thead>
                <tbody>
                  {displayDocs.map((doc, i) => (
                    <tr
                      key={doc.id}
                      onClick={() => setSelected(doc.id === selected?.id ? null : doc)}
                      className={`cursor-pointer border-b border-surface2/30 transition-colors hover:bg-surface/40 ${
                        selected?.id === doc.id ? 'bg-primary/10' : ''
                      }`}
                    >
                      <td className="px-3 py-2">
                        <p className="max-w-xs truncate font-medium text-white">
                          {doc.title ?? doc.source_path.split('/').pop()}
                        </p>
                        {searchResults?.[i]?.snippet && (
                          <p className="mt-0.5 max-w-xs truncate text-xs text-gray-500">
                            {searchResults[i].snippet}
                          </p>
                        )}
                      </td>
                      <td className="px-3 py-2 text-gray-400">{doc.source_type}</td>
                      <td className="px-3 py-2 text-gray-400">{doc.domain ?? '—'}</td>
                      <td className="px-3 py-2 text-gray-400">{fmtWords(doc.word_count)}</td>
                      <td className="px-3 py-2 text-gray-400">{fmtTimestamp(doc.indexed_at)}</td>
                      <td className="px-3 py-2">
                        <button
                          onClick={(e) => { e.stopPropagation(); void handleDelete(doc); }}
                          disabled={deleting === doc.id}
                          className="rounded px-2 py-1 text-xs text-gray-500 hover:bg-red-500/20 hover:text-red-400 disabled:opacity-40"
                        >
                          {deleting === doc.id ? '…' : 'Delete'}
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>

        {/* Right: preview + ingest */}
        <div className="w-full lg:w-96 flex flex-col gap-4">
          {/* Preview drawer */}
          {selected && (
            <div className="rounded-xl border border-surface2/60 bg-surface/60 p-4">
              <div className="mb-2 flex items-center justify-between">
                <h2 className="text-sm font-semibold text-white truncate">
                  {selected.title ?? selected.source_path.split('/').pop()}
                </h2>
                <button onClick={() => setSelected(null)} className="text-xs text-gray-500 hover:text-white ml-2">✕</button>
              </div>
              <dl className="mb-3 grid grid-cols-2 gap-x-2 gap-y-1 text-xs">
                <MetaRow label="ID" value={selected.id.slice(0, 8) + '…'} />
                <MetaRow label="Source" value={selected.source_type} />
                <MetaRow label="Domain" value={selected.domain ?? '—'} />
                <MetaRow label="Words" value={String(selected.word_count ?? 0)} />
                <MetaRow label="MIME" value={selected.mime_type ?? '—'} />
                <MetaRow label="Accesses" value={String(selected.access_count)} />
                <MetaRow label="Indexed" value={fmtTimestamp(selected.indexed_at)} />
                {selected.last_accessed && (
                  <MetaRow label="Last read" value={fmtTimestamp(selected.last_accessed)} />
                )}
              </dl>
              <p className="text-xs text-gray-500">Path</p>
              <p className="mb-3 break-all text-xs text-gray-300">{selected.source_path}</p>
              <p className="text-xs text-gray-500">Content preview</p>
              <pre className="mt-1 max-h-48 overflow-y-auto whitespace-pre-wrap break-words rounded-lg border border-surface2/40 bg-background p-2 text-xs text-gray-300">
                {selected.content.slice(0, 2000)}
                {selected.content.length > 2000 && (
                  <span className="text-gray-500">… ({selected.content.length} chars)</span>
                )}
              </pre>
            </div>
          )}

          {/* Ingest panel */}
          <div className="rounded-xl border border-surface2/60 bg-surface/60 p-4">
            <h2 className="mb-3 text-sm font-semibold text-white">Ingest</h2>

            <div className="mb-3 flex flex-wrap gap-1 text-xs">
              {(['upload', 'file', 'folder', 'url', 'sitemap'] as IngestTab[]).map((tab) => (
                <button
                  key={tab}
                  onClick={() => { setIngestTab(tab); setIngestResult(null); setActiveJob(null); }}
                  className={`rounded px-2 py-1 capitalize transition-colors ${
                    ingestTab === tab ? 'bg-primary/90 text-white' : 'text-gray-400 hover:text-white'
                  }`}
                >
                  {tab}
                </button>
              ))}
            </div>

            {/* Upload tab */}
            {ingestTab === 'upload' && (
              <div className="space-y-3">
                <div
                  onDragOver={onDragOver}
                  onDragLeave={onDragLeave}
                  onDrop={onDrop}
                  onClick={() => fileInputRef.current?.click()}
                  className={`flex cursor-pointer flex-col items-center justify-center gap-2 rounded-xl border-2 border-dashed px-4 py-8 transition-colors ${
                    dragOver
                      ? 'border-primary bg-primary/10 text-primary'
                      : 'border-surface2 text-gray-500 hover:border-gray-500 hover:text-gray-300'
                  }`}
                >
                  <span className="text-2xl">⊕</span>
                  <p className="text-center text-xs">
                    Drag files here or <span className="underline">click to browse</span>
                  </p>
                  <p className="text-center text-xs text-gray-600">
                    {SUPPORTED_EXTENSIONS} · max 25 MiB each
                  </p>
                </div>
                <input
                  ref={fileInputRef}
                  type="file"
                  multiple
                  accept={SUPPORTED_EXTENSIONS.split(' ').map((e) => `.${e}`).join(',')}
                  onChange={onFileInput}
                  className="hidden"
                />
                {uploadFiles.length > 0 && (
                  <ul className="space-y-1 rounded-lg border border-surface2/40 bg-background p-2">
                    {uploadFiles.map((f) => (
                      <li key={f.name} className="flex items-center justify-between text-xs">
                        <span className="truncate text-gray-300">{f.name}</span>
                        <span className="ml-2 shrink-0 text-gray-600">{(f.size / 1024).toFixed(0)} KB</span>
                        <button
                          onClick={() => setUploadFiles((prev) => prev.filter((x) => x.name !== f.name))}
                          className="ml-2 shrink-0 text-gray-600 hover:text-red-400"
                        >✕</button>
                      </li>
                    ))}
                  </ul>
                )}
                <button
                  onClick={() => void handleUpload()}
                  disabled={ingesting || uploadFiles.length === 0}
                  className="w-full rounded-lg bg-primary/80 px-3 py-2 text-xs font-semibold text-white hover:bg-primary disabled:cursor-not-allowed disabled:opacity-40"
                >
                  {ingesting ? 'Uploading…' : uploadFiles.length ? `Upload ${uploadFiles.length} file${uploadFiles.length > 1 ? 's' : ''}` : 'Upload'}
                </button>
              </div>
            )}

            {/* File / Folder */}
            {(ingestTab === 'file' || ingestTab === 'folder') && (
              <div className="mb-2">
                <label className="mb-1 block text-xs text-gray-500">Path (inside workspace)</label>
                <input
                  type="text"
                  value={ingestPath}
                  onChange={(e) => setIngestPath(e.target.value)}
                  placeholder="/workspace/docs"
                  className="w-full rounded border border-surface2 bg-background px-2 py-1.5 text-xs text-white placeholder-gray-600 focus:outline-none focus:ring-1 focus:ring-primary"
                />
              </div>
            )}

            {/* URL / Sitemap */}
            {(ingestTab === 'url' || ingestTab === 'sitemap') && (
              <div className="mb-2">
                <label className="mb-1 block text-xs text-gray-500">URL</label>
                <input
                  type="text"
                  value={ingestUrl}
                  onChange={(e) => setIngestUrl(e.target.value)}
                  placeholder="https://example.com"
                  className="w-full rounded border border-surface2 bg-background px-2 py-1.5 text-xs text-white placeholder-gray-600 focus:outline-none focus:ring-1 focus:ring-primary"
                />
              </div>
            )}

            {ingestTab !== 'upload' && (
              <>
                <div className="mb-2 flex gap-2">
                  <div className="flex-1">
                    <label className="mb-1 block text-xs text-gray-500">Domain</label>
                    <input
                      type="text"
                      value={ingestDomain}
                      onChange={(e) => setIngestDomain(e.target.value)}
                      placeholder="docs"
                      className="w-full rounded border border-surface2 bg-background px-2 py-1.5 text-xs text-white placeholder-gray-600 focus:outline-none focus:ring-1 focus:ring-primary"
                    />
                  </div>
                  <div className="flex-1">
                    <label className="mb-1 block text-xs text-gray-500">Tags</label>
                    <input
                      type="text"
                      value={ingestTags}
                      onChange={(e) => setIngestTags(e.target.value)}
                      placeholder="api, ref"
                      className="w-full rounded border border-surface2 bg-background px-2 py-1.5 text-xs text-white placeholder-gray-600 focus:outline-none focus:ring-1 focus:ring-primary"
                    />
                  </div>
                </div>
                <div className="mb-3 flex gap-4 text-xs">
                  <label className="flex items-center gap-1.5 text-gray-400">
                    <input type="checkbox" checked={ingestForce} onChange={(e) => setIngestForce(e.target.checked)} className="accent-primary" />
                    Force re-index
                  </label>
                  {(ingestTab === 'folder' || ingestTab === 'sitemap') && (
                    <label className="flex items-center gap-1.5 text-gray-400">
                      <input type="checkbox" checked={ingestDryRun} onChange={(e) => setIngestDryRun(e.target.checked)} className="accent-primary" />
                      Dry run
                    </label>
                  )}
                </div>
                <button
                  onClick={() => void handleIngest()}
                  disabled={
                    ingesting ||
                    ((ingestTab === 'file' || ingestTab === 'folder') && !ingestPath.trim()) ||
                    ((ingestTab === 'url' || ingestTab === 'sitemap') && !ingestUrl.trim())
                  }
                  className="w-full rounded-lg bg-primary/80 px-3 py-2 text-xs font-semibold text-white hover:bg-primary disabled:cursor-not-allowed disabled:opacity-40"
                >
                  {ingesting ? 'Working…' : 'Ingest'}
                </button>
              </>
            )}

            {/* Active background job */}
            {activeJob && (
              <div className={`mt-3 rounded-lg border p-3 text-xs ${
                activeJob.status === 'running'
                  ? 'border-yellow-500/30 bg-yellow-500/5'
                  : activeJob.status === 'done'
                  ? 'border-green-500/30 bg-green-500/5'
                  : 'border-red-500/30 bg-red-500/5'
              }`}>
                <div className="flex items-center justify-between">
                  <span className={`font-semibold ${
                    activeJob.status === 'running' ? 'text-yellow-400'
                    : activeJob.status === 'done' ? 'text-green-400'
                    : 'text-red-400'
                  }`}>
                    {activeJob.status === 'running' ? '⟳ Running…' : activeJob.status === 'done' ? '✓ Done' : '✗ Error'}
                  </span>
                  <span className="text-gray-500">{activeJob.processed}/{activeJob.total}</span>
                </div>
                <p className="mt-1 truncate text-gray-500">{activeJob.source}</p>
                {activeJob.errors.length > 0 && (
                  <ul className="mt-1 space-y-0.5 text-red-400">
                    {activeJob.errors.slice(0, 3).map((e, i) => <li key={i} className="truncate">{e}</li>)}
                    {activeJob.errors.length > 3 && <li className="text-gray-500">+{activeJob.errors.length - 3} more</li>}
                  </ul>
                )}
              </div>
            )}

            {/* Ingest result */}
            {ingestResult && (
              <div className="mt-3 rounded-lg border border-surface2/40 bg-background p-3 text-xs">
                {'total' in ingestResult ? (
                  <IngestSummaryView summary={ingestResult} />
                ) : (
                  <IngestResultView result={ingestResult} />
                )}
              </div>
            )}
          </div>
        </div>
      </div>
    </main>
  );
}

function StatBadge({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-surface2/60 bg-surface/60 px-3 py-2 text-center">
      <p className="text-xs text-gray-500">{label}</p>
      <p className="text-sm font-semibold text-white">{value}</p>
    </div>
  );
}

function MetaRow({ label, value }: { label: string; value: string }) {
  return (
    <>
      <dt className="text-gray-500">{label}</dt>
      <dd className="text-gray-300">{value}</dd>
    </>
  );
}

function IngestResultView({ result }: { result: KnowledgeIngestResult }) {
  return (
    <div className="text-green-400">
      <p className="font-semibold">Ingested</p>
      <p className="text-gray-400">{result.title ?? result.source_path}</p>
      <p className="text-gray-500">{result.word_count} words · {result.source_type}</p>
    </div>
  );
}

function IngestSummaryView({ summary }: { summary: KnowledgeIngestSummary }) {
  return (
    <div>
      <p className="font-semibold text-white">
        {summary.ingested.length}/{summary.total} ingested
        {summary.skipped.length > 0 && `, ${summary.skipped.length} skipped`}
        {summary.errors.length > 0 && `, ${summary.errors.length} errors`}
      </p>
      {summary.ingested.length > 0 && (
        <ul className="mt-1 space-y-0.5 text-green-400">
          {summary.ingested.map((r) => (
            <li key={r.id} className="truncate">{r.title ?? r.source_path} · {r.word_count}w</li>
          ))}
        </ul>
      )}
      {summary.errors.length > 0 && (
        <ul className="mt-1 space-y-0.5 text-red-400">
          {summary.errors.map((e, i) => <li key={i} className="truncate">{e}</li>)}
        </ul>
      )}
    </div>
  );
}
