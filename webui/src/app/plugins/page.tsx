'use client';

import { useEffect } from 'react';

import Nav from '@/components/Nav';
import { useRoveStore } from '@/stores/roveStore';

export default function PluginsPage() {
  const {
    clearError,
    error,
    extensions,
    initialize,
    removeExtension,
    setExtensionEnabled,
  } = useRoveStore();

  useEffect(() => {
    void initialize();
  }, [initialize]);

  return (
    <div className="min-h-screen flex flex-col">
      <header className="sticky top-0 z-10 bg-background/80 backdrop-blur border-b border-surface2">
        <div className="max-w-4xl mx-auto px-4 py-4">
          <div className="flex items-center gap-3 mb-4">
            <span className="text-3xl">🌐</span>
            <h1 className="text-2xl font-bold bg-gradient-to-r from-primary to-purple-500 bg-clip-text text-transparent">
              Rove
            </h1>
          </div>
          <Nav />
        </div>
      </header>

      <main className="flex-1 max-w-4xl w-full mx-auto px-4 py-6 space-y-6">
        <section className="bg-surface rounded-xl p-6 border border-surface2">
          <div className="flex items-center justify-between mb-6">
            <h2 className="text-xl font-semibold flex items-center gap-2">
              <span>▣</span> Extensions
            </h2>
          </div>

          <div className="space-y-4">
            {extensions.length === 0 ? (
              <div className="rounded-lg bg-surface2 p-4 text-sm text-gray-400">
                No enabled extensions reported by the local daemon.
              </div>
            ) : (
              extensions.map((extension) => (
                <ExtensionCard
                  key={`${extension.kind}:${extension.id}`}
                  extension={extension}
                  onEnable={() => void setExtensionEnabled(extension.kind, extension.id, true)}
                  onDisable={() => void setExtensionEnabled(extension.kind, extension.id, false)}
                  onRemove={() => void removeExtension(extension.kind, extension.id)}
                />
              ))
            )}
          </div>
          <ErrorBanner error={error} onDismiss={clearError} />
        </section>
      </main>

      <footer className="border-t border-surface2 py-4 text-center text-sm text-gray-500">
        Rove v0.0.3 — Local-first AI Agent
      </footer>
    </div>
  );
}

function ExtensionCard({
  extension,
  onEnable,
  onDisable,
  onRemove,
}: {
  extension: {
    id: string;
    name: string;
    kind: string;
    state: string;
    source: string;
    description: string;
    version?: string | null;
    official: boolean;
  };
  onEnable: () => void;
  onDisable: () => void;
  onRemove: () => void;
}) {
  const canInstall = extension.official && extension.state === 'available';
  const isEnabled = extension.state === 'installed';

  return (
    <div className="flex items-center justify-between p-4 bg-surface2 rounded-lg">
      <div className="flex-1">
        <div className="flex items-center gap-3">
          <p className="font-medium">{extension.name}</p>
          <span className="text-xs text-gray-500 uppercase">{extension.kind}</span>
          <span className={`px-2 py-0.5 rounded text-xs ${
            extension.state === 'installed'
              ? 'bg-success/20 text-success'
              : extension.state === 'installed-disabled'
                ? 'bg-warning/20 text-warning'
                : 'bg-surface text-gray-400'
          }`}>
            {extension.state}
          </span>
        </div>
        <p className="text-sm text-gray-500 mt-1">{extension.description}</p>
        {extension.version && <p className="mt-1 text-xs text-gray-500">version {extension.version}</p>}
      </div>
      <div className="flex gap-2">
        {canInstall ? (
          <button onClick={onEnable} className="rounded-lg bg-primary px-4 py-2 text-sm hover:bg-primary/80">
            Install
          </button>
        ) : isEnabled ? (
          <button onClick={onDisable} className="rounded-lg border border-surface px-4 py-2 text-sm hover:border-primary">
            Disable
          </button>
        ) : (
          <button onClick={onEnable} className="rounded-lg bg-primary px-4 py-2 text-sm hover:bg-primary/80">
            Enable
          </button>
        )}
        {extension.state !== 'available' && (
          <button onClick={onRemove} className="rounded-lg border border-error/30 px-4 py-2 text-sm text-error hover:bg-error/10">
            Remove
          </button>
        )}
      </div>
    </div>
  );
}

function ErrorBanner({ error, onDismiss }: { error: string | null; onDismiss: () => void }) {
  if (!error) return null;
  return (
    <div className="mt-4 rounded-lg border border-error/30 bg-error/10 px-4 py-3 text-sm text-error">
      <div className="flex items-start justify-between gap-3">
        <p>{error}</p>
        <button onClick={onDismiss}>×</button>
      </div>
    </div>
  );
}
