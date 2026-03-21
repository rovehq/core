'use client';

import { useEffect } from 'react';

import Nav from '@/components/Nav';
import { useRoveStore } from '@/stores/roveStore';

export default function ConfigPage() {
  const { appState, daemonUrl, hello, initialize } = useRoveStore();

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
          <h2 className="text-xl font-semibold mb-6 flex items-center gap-2">
            <span>⚙</span> Daemon Configuration
          </h2>

          <div className="space-y-4">
            <div className="p-4 bg-surface2 rounded-lg">
              <h3 className="font-medium mb-2">Daemon Reachability</h3>
              <p className="text-sm text-gray-300">
                State: <span className="font-mono">{appState}</span>
              </p>
              <p className="text-sm text-gray-500 mt-2">
                Endpoint: <code className="bg-background px-2 py-0.5 rounded">{daemonUrl ?? 'not discovered'}</code>
              </p>
            </div>

            <div className="p-4 bg-surface2 rounded-lg">
              <h3 className="font-medium mb-4">Node Identity</h3>
              <div className="space-y-3">
                <SettingRow label="Node" value={hello?.node.node_name ?? 'unknown'} />
                <SettingRow label="Role" value={hello?.node.role ?? 'unknown'} />
                <SettingRow label="Version" value={hello?.version ?? 'unknown'} />
              </div>
            </div>

            <div className="p-4 bg-surface2 rounded-lg">
              <h3 className="font-medium mb-4">Installed Capabilities</h3>
              <div className="space-y-3 text-sm">
                <SettingRow label="Brains" value={hello?.capabilities.brains.join(', ') || 'none'} />
                <SettingRow label="Services" value={hello?.capabilities.services.join(', ') || 'none'} />
                <SettingRow label="Extensions" value={String(hello?.capabilities.extensions.length ?? 0)} />
              </div>
            </div>
          </div>
        </section>
      </main>

      <footer className="border-t border-surface2 py-4 text-center text-sm text-gray-500">
        Rove v0.0.3 — Local-first AI Agent
      </footer>
    </div>
  );
}

function SettingRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between">
      <span className="text-gray-400">{label}</span>
      <span className="font-mono text-sm">{value}</span>
    </div>
  );
}
