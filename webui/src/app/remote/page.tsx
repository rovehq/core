'use client';

import { useEffect, useState } from 'react';

import Nav from '@/components/Nav';
import { useRoveStore } from '@/stores/roveStore';

export default function RemotePage() {
  const {
    clearError,
    error,
    initialize,
    refreshRemote,
    refreshZeroTier,
    remoteNodes,
    remoteStatus,
    trustRemoteNode,
    unpairRemoteNode,
    zeroTier,
    joinZeroTier,
  } = useRoveStore();
  const [networkId, setNetworkId] = useState('');

  useEffect(() => {
    void initialize();
    void refreshRemote();
    void refreshZeroTier();
  }, [initialize, refreshRemote, refreshZeroTier]);

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
          <Stat label="Node ID" value={remoteStatus?.node.node_id ?? 'unknown'} />
          <Stat label="Role" value={remoteStatus?.profile.execution_role ?? 'unknown'} />
          <Stat label="Queue" value={`${remoteStatus?.load?.pending_tasks ?? 0} pending / ${remoteStatus?.load?.running_tasks ?? 0} running`} />
          <Stat label="Recent" value={`${remoteStatus?.load?.recent_successes ?? 0} ok / ${remoteStatus?.load?.recent_failures ?? 0} fail`} />
        </section>

        <section className="bg-surface rounded-xl p-6 border border-surface2 space-y-4">
          <div className="flex items-center justify-between">
            <div>
              <h2 className="text-lg font-semibold">ZeroTier Transport</h2>
              <p className="text-sm text-gray-400">
                {zeroTier?.enabled ? 'enabled' : 'disabled'} · service {zeroTier?.service_online ? 'online' : 'offline'}
              </p>
            </div>
            <button
              onClick={() => void refreshZeroTier()}
              className="rounded-lg border border-surface2 px-3 py-2 text-sm hover:border-primary"
            >
              Refresh Transport
            </button>
          </div>
          <div className="rounded-lg bg-surface2 px-4 py-3 text-sm text-gray-300 space-y-1">
            <p>Network: <span className="font-mono">{zeroTier?.network_id ?? 'not configured'}</span></p>
            <p>Node: <span className="font-mono">{zeroTier?.node_id ?? 'unknown'}</span></p>
            <p>Assigned addresses: {zeroTier?.assigned_addresses.join(', ') || 'none'}</p>
            <p>Transport URLs: {zeroTier?.transport_records.map((record) => record.base_url ?? record.address).join(', ') || 'none'}</p>
            {zeroTier?.message ? <p className="text-gray-500">{zeroTier.message}</p> : null}
          </div>
          <form
            className="flex flex-col gap-3 md:flex-row"
            onSubmit={async (event) => {
              event.preventDefault();
              await joinZeroTier(networkId || zeroTier?.network_id || undefined);
            }}
          >
            <input
              value={networkId}
              onChange={(event) => setNetworkId(event.target.value)}
              className="flex-1 rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
              placeholder="ZeroTier network id"
            />
            <button className="rounded-lg bg-primary px-4 py-2 text-sm font-medium hover:bg-primary/80">
              Join Network
            </button>
          </form>
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
                      <p className="text-sm text-gray-500">id {node.identity.node_id}</p>
                      <p className="text-sm text-gray-500">{node.target}</p>
                      <p className="text-sm text-gray-500">
                        {node.profile.execution_role} · tags {node.profile.tags.join(', ') || 'none'} · caps {node.profile.capabilities.join(', ') || 'none'}
                      </p>
                      <p className="text-sm text-gray-500">
                        transports {node.transports.map((record) => record.base_url ?? record.address).join(', ') || 'none'}
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
