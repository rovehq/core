'use client';

import { useEffect } from 'react';

import Nav from '@/components/Nav';
import { useRoveStore } from '@/stores/roveStore';

export default function RemotePage() {
  const {
    clearError,
    error,
    initialize,
    refreshRemote,
    remoteNodes,
    remoteStatus,
    trustRemoteNode,
    unpairRemoteNode,
  } = useRoveStore();

  useEffect(() => {
    void initialize();
    void refreshRemote();
  }, [initialize, refreshRemote]);

  return (
    <div className="min-h-screen flex flex-col">
      <header className="sticky top-0 z-10 bg-background/80 backdrop-blur border-b border-surface2">
        <div className="max-w-6xl mx-auto px-4 py-4 space-y-4">
          <div>
            <h1 className="text-2xl font-semibold">Remote</h1>
            <p className="text-sm text-gray-400">Inspect local node load and manage paired daemon nodes in the mesh.</p>
          </div>
          <Nav />
        </div>
      </header>

      <main className="flex-1 max-w-6xl w-full mx-auto px-4 py-6 space-y-6">
        <section className="bg-surface rounded-xl p-6 border border-surface2 grid gap-4 md:grid-cols-2 xl:grid-cols-4">
          <Stat label="Node" value={remoteStatus?.node.node_name ?? 'unknown'} />
          <Stat label="Role" value={remoteStatus?.profile.execution_role ?? 'unknown'} />
          <Stat label="Queue" value={`${remoteStatus?.load?.pending_tasks ?? 0} pending / ${remoteStatus?.load?.running_tasks ?? 0} running`} />
          <Stat label="Recent" value={`${remoteStatus?.load?.recent_successes ?? 0} ok / ${remoteStatus?.load?.recent_failures ?? 0} fail`} />
        </section>

        <section className="bg-surface rounded-xl p-6 border border-surface2 space-y-4">
          <div className="flex items-center justify-between">
            <h2 className="text-lg font-semibold">Paired Nodes</h2>
            <button
              onClick={() => void refreshRemote()}
              className="rounded-lg border border-surface2 px-3 py-2 text-sm hover:border-primary"
            >
              Refresh
            </button>
          </div>
          {remoteNodes.length === 0 ? (
            <p className="text-sm text-gray-400">No remote nodes are paired yet.</p>
          ) : (
            <div className="space-y-3">
              {remoteNodes.map((node) => (
                <div key={node.identity.node_id} className="rounded-lg bg-surface2 px-4 py-3">
                  <div className="flex items-start justify-between gap-4">
                    <div>
                      <p className="font-medium">{node.identity.node_name}</p>
                      <p className="text-sm text-gray-500">{node.target}</p>
                      <p className="text-sm text-gray-500">
                        {node.profile.execution_role} · tags {node.profile.tags.join(', ') || 'none'} · caps {node.profile.capabilities.join(', ') || 'none'}
                      </p>
                    </div>
                    <div className="flex items-center gap-2">
                      {!node.trusted && (
                        <button
                          onClick={() => void trustRemoteNode(node.identity.node_name)}
                          className="rounded-lg bg-primary px-3 py-2 text-sm hover:bg-primary/80"
                        >
                          Trust
                        </button>
                      )}
                      <button
                        onClick={() => void unpairRemoteNode(node.identity.node_name)}
                        className="rounded-lg border border-error/30 px-3 py-2 text-sm text-error hover:bg-error/10"
                      >
                        Unpair
                      </button>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
          <ErrorBanner error={error} onDismiss={clearError} />
        </section>
      </main>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg bg-surface2 px-4 py-3">
      <p className="text-sm text-gray-500">{label}</p>
      <p className="mt-1 font-medium">{value}</p>
    </div>
  );
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
