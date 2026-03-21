'use client';

import { useEffect } from 'react';

import Nav from '@/components/Nav';
import { useRoveStore } from '@/stores/roveStore';

export default function PluginsPage() {
  const { hello, initialize } = useRoveStore();

  useEffect(() => {
    void initialize();
  }, [initialize]);

  const extensions = hello?.capabilities.extensions ?? [];

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
                <ExtensionCard key={extension} extension={extension} />
              ))
            )}
          </div>
        </section>
      </main>

      <footer className="border-t border-surface2 py-4 text-center text-sm text-gray-500">
        Rove v0.0.3 — Local-first AI Agent
      </footer>
    </div>
  );
}

function ExtensionCard({ extension }: { extension: string }) {
  const [kind, name] = extension.split(':', 2);

  return (
    <div className="flex items-center justify-between p-4 bg-surface2 rounded-lg">
      <div className="flex-1">
        <div className="flex items-center gap-3">
          <p className="font-medium">{name ?? extension}</p>
          <span className="text-xs text-gray-500 uppercase">{kind ?? 'extension'}</span>
          <span className="px-2 py-0.5 rounded text-xs bg-success/20 text-success">
            enabled
          </span>
        </div>
        <p className="text-sm text-gray-500 mt-1">{extension}</p>
      </div>
    </div>
  );
}
