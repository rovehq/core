'use client';

import { useEffect } from 'react';

import Nav from '@/components/Nav';
import { useRoveStore } from '@/stores/roveStore';

export default function BrainsPage() {
  const { brains, clearError, error, initialize, refreshBrains, useDispatchBrain } = useRoveStore();

  useEffect(() => {
    void initialize();
    void refreshBrains();
  }, [initialize, refreshBrains]);

  const dispatch = brains?.dispatch;

  return (
    <div className="min-h-screen flex flex-col">
      <header className="sticky top-0 z-10 bg-background/80 backdrop-blur border-b border-surface2">
        <div className="max-w-5xl mx-auto px-4 py-4 space-y-4">
          <div>
            <h1 className="text-2xl font-semibold">Brains</h1>
            <p className="text-sm text-gray-400">Manage local dispatch models and the active brain family selection.</p>
          </div>
          <Nav />
        </div>
      </header>

      <main className="flex-1 max-w-5xl w-full mx-auto px-4 py-6 space-y-6">
        <section className="bg-surface rounded-xl p-6 border border-surface2 space-y-3">
          <h2 className="text-lg font-semibold">Dispatch Brain</h2>
          <p className="text-sm text-gray-400">Root: {dispatch?.root ?? 'not available'}</p>
          <p className="text-sm text-gray-400">Source: {dispatch?.source ?? 'not installed'}</p>
          <p className="text-sm text-gray-400">Active: {dispatch?.active ?? 'none selected'}</p>
        </section>

        <section className="bg-surface rounded-xl p-6 border border-surface2 space-y-4">
          <div className="flex items-center justify-between">
            <h2 className="text-lg font-semibold">Installed Dispatch Models</h2>
            <button
              onClick={() => void refreshBrains()}
              className="rounded-lg border border-surface2 px-3 py-2 text-sm hover:border-primary"
            >
              Refresh
            </button>
          </div>
          {dispatch?.installed?.length ? (
            <div className="space-y-3">
              {dispatch.installed.map((model) => {
                const active = model === dispatch.active;
                return (
                  <div key={model} className="flex items-center justify-between rounded-lg bg-surface2 px-4 py-3">
                    <div>
                      <p className="font-medium">{model}</p>
                      <p className="text-sm text-gray-500">{active ? 'Currently active dispatch brain' : 'Installed and ready'}</p>
                    </div>
                    <button
                      disabled={active}
                      onClick={() => void useDispatchBrain(model)}
                      className="rounded-lg bg-primary px-4 py-2 text-sm hover:bg-primary/80 disabled:bg-surface disabled:text-gray-500"
                    >
                      {active ? 'Active' : 'Use'}
                    </button>
                  </div>
                );
              })}
            </div>
          ) : (
            <p className="text-sm text-gray-400">No dispatch models are installed yet. Install one with the CLI or local artifact flow.</p>
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
