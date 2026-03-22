'use client';

import { useEffect } from 'react';

import Nav from '@/components/Nav';
import { useRoveStore } from '@/stores/roveStore';

export default function ApprovalsPage() {
  const { approvals, clearError, error, initialize, refreshApprovals, resolveApproval } = useRoveStore();

  useEffect(() => {
    void initialize();
    void refreshApprovals();
  }, [initialize, refreshApprovals]);

  return (
    <div className="min-h-screen flex flex-col">
      <header className="sticky top-0 z-10 bg-background/80 backdrop-blur border-b border-surface2">
        <div className="max-w-5xl mx-auto px-4 py-4 space-y-4">
          <div>
            <h1 className="text-2xl font-semibold">Approvals</h1>
            <p className="text-sm text-gray-400">Resolve daemon-held risk prompts for WebUI and background task sessions.</p>
          </div>
          <Nav />
        </div>
      </header>

      <main className="flex-1 max-w-5xl w-full mx-auto px-4 py-6 space-y-6">
        <section className="bg-surface rounded-xl p-6 border border-surface2 space-y-4">
          <div className="flex items-center justify-between">
            <h2 className="text-lg font-semibold">Pending Approvals</h2>
            <button
              onClick={() => void refreshApprovals()}
              className="rounded-lg border border-surface2 px-3 py-2 text-sm hover:border-primary"
            >
              Refresh
            </button>
          </div>
          {approvals.length === 0 ? (
            <p className="text-sm text-gray-400">No pending approvals.</p>
          ) : (
            <div className="space-y-3">
              {approvals.map((approval) => (
                <div key={approval.id} className="rounded-lg bg-surface2 px-4 py-3">
                  <div className="flex items-start justify-between gap-4">
                    <div>
                      <p className="font-medium">Tier {approval.risk_tier} · {approval.tool_name}</p>
                      <p className="text-sm text-gray-500">Task {approval.task_id}</p>
                      <p className="mt-2 text-sm text-gray-300">{approval.summary}</p>
                      {approval.auto_resolve_after_secs ? (
                        <p className="mt-1 text-xs text-gray-500">
                          Auto resolves in {approval.auto_resolve_after_secs}s if left untouched.
                        </p>
                      ) : null}
                    </div>
                    <div className="flex items-center gap-2">
                      <button
                        onClick={() => void resolveApproval(approval.id, true)}
                        className="rounded-lg bg-primary px-3 py-2 text-sm hover:bg-primary/80"
                      >
                        Approve
                      </button>
                      <button
                        onClick={() => void resolveApproval(approval.id, false)}
                        className="rounded-lg border border-error/30 px-3 py-2 text-sm text-error hover:bg-error/10"
                      >
                        Deny
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
