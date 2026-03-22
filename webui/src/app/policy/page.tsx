'use client';

import { useEffect, useState } from 'react';

import Nav from '@/components/Nav';
import { useRoveStore } from '@/stores/roveStore';

export default function PolicyPage() {
  const {
    addPolicy,
    clearError,
    error,
    explainPolicy,
    initialize,
    policies,
    policyExplain,
    refreshPolicies,
    removePolicy,
    setPolicyEnabled,
  } = useRoveStore();
  const [newPolicyName, setNewPolicyName] = useState('');
  const [newPolicyScope, setNewPolicyScope] = useState<'user' | 'workspace' | 'project'>('workspace');
  const [task, setTask] = useState('refactor this module safely');

  useEffect(() => {
    void initialize();
    void refreshPolicies();
  }, [initialize, refreshPolicies]);

  return (
    <div className="min-h-screen flex flex-col">
      <header className="sticky top-0 z-10 bg-background/80 backdrop-blur border-b border-surface2">
        <div className="max-w-6xl mx-auto px-4 py-4 space-y-4">
          <div>
            <h1 className="text-2xl font-semibold">Policy</h1>
            <p className="text-sm text-gray-400">Inspect, enable, and explain the execution policies shaping Rove behavior.</p>
          </div>
          <Nav />
        </div>
      </header>

      <main className="flex-1 max-w-6xl w-full mx-auto px-4 py-6 grid gap-6 lg:grid-cols-[1.2fr,0.8fr]">
        <section className="bg-surface rounded-xl p-6 border border-surface2 space-y-4">
          <div className="flex items-center justify-between">
            <h2 className="text-lg font-semibold">Policy Files</h2>
            <button
              onClick={() => void refreshPolicies()}
              className="rounded-lg border border-surface2 px-3 py-2 text-sm hover:border-primary"
            >
              Refresh
            </button>
          </div>
          <div className="space-y-3">
            {policies.map((policy) => (
              <div key={policy.id} className="rounded-lg bg-surface2 px-4 py-3">
                <div className="flex items-start justify-between gap-4">
                  <div>
                    <p className="font-medium">{policy.id}</p>
                    <p className="text-sm text-gray-500">{policy.scope} · {policy.path}</p>
                  </div>
                  <div className="flex items-center gap-2">
                    <button
                      onClick={() => void setPolicyEnabled(policy.id, !policy.active)}
                      className="rounded-lg bg-primary px-3 py-2 text-sm hover:bg-primary/80"
                    >
                      {policy.active ? 'Disable' : 'Enable'}
                    </button>
                    <button
                      onClick={() => void removePolicy(policy.id)}
                      className="rounded-lg border border-error/30 px-3 py-2 text-sm text-error hover:bg-error/10"
                    >
                      Remove
                    </button>
                  </div>
                </div>
              </div>
            ))}
          </div>
        </section>

        <section className="space-y-6">
          <section className="bg-surface rounded-xl p-6 border border-surface2 space-y-4">
            <h2 className="text-lg font-semibold">Create Policy</h2>
            <input
              value={newPolicyName}
              onChange={(event) => setNewPolicyName(event.target.value)}
              placeholder="rust-safe"
              className="w-full rounded-lg border border-surface2 bg-background px-3 py-3 outline-none focus:border-primary"
            />
            <select
              value={newPolicyScope}
              onChange={(event) => setNewPolicyScope(event.target.value as 'user' | 'workspace' | 'project')}
              className="w-full rounded-lg border border-surface2 bg-background px-3 py-3 outline-none focus:border-primary"
            >
              <option value="workspace">Workspace</option>
              <option value="user">User</option>
              <option value="project">Project</option>
            </select>
            <button
              onClick={() => {
                if (newPolicyName.trim()) {
                  void addPolicy(newPolicyName.trim(), newPolicyScope).then((ok) => {
                    if (ok) setNewPolicyName('');
                  });
                }
              }}
              className="rounded-lg bg-primary px-4 py-2 text-sm hover:bg-primary/80"
            >
              Create Policy
            </button>
          </section>

          <section className="bg-surface rounded-xl p-6 border border-surface2 space-y-4">
            <h2 className="text-lg font-semibold">Explain Active Policy</h2>
            <textarea
              value={task}
              onChange={(event) => setTask(event.target.value)}
              className="h-28 w-full rounded-lg border border-surface2 bg-background px-3 py-3 outline-none focus:border-primary"
            />
            <button
              onClick={() => void explainPolicy(task)}
              className="rounded-lg bg-primary px-4 py-2 text-sm hover:bg-primary/80"
            >
              Explain
            </button>
            {policyExplain && (
              <div className="rounded-lg bg-surface2 p-4 text-sm space-y-2">
                <p><span className="text-gray-400">Domain:</span> {policyExplain.domain}</p>
                <p><span className="text-gray-400">Active:</span> {policyExplain.active_policies.join(', ') || 'none'}</p>
                <p><span className="text-gray-400">Hints:</span> {policyExplain.matched_hints.join(', ') || 'none'}</p>
                <p><span className="text-gray-400">Providers:</span> {policyExplain.preferred_providers.join(', ') || 'none'}</p>
                <p><span className="text-gray-400">Tools:</span> {policyExplain.preferred_tools.join(', ') || 'none'}</p>
                <p><span className="text-gray-400">Verify:</span> {policyExplain.verification_commands.join(', ') || 'none'}</p>
              </div>
            )}
          </section>
          <ErrorBanner error={error} onDismiss={clearError} />
        </section>
      </main>
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
