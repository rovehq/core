'use client';

import { useEffect, useState } from 'react';

import { DEFAULT_DAEMON_PORT } from '@/lib/daemon';
import Nav from '@/components/Nav';
import { useRoveStore } from '@/stores/roveStore';

export default function SettingsPage() {
  const {
    appState,
    authStatus,
    clearError,
    config,
    daemonPort,
    daemonUrl,
    error,
    hello,
    initialize,
    installService,
    lock,
    refreshServiceInstall,
    setDaemonPort,
    updateConfig,
    serviceInstall,
    services,
    setServiceEnabled,
    uninstallService,
  } = useRoveStore();
  const [portInput, setPortInput] = useState(String(DEFAULT_DAEMON_PORT));

  useEffect(() => {
    void initialize();
    void refreshServiceInstall();
  }, [initialize, refreshServiceInstall]);

  useEffect(() => {
    setPortInput(String(daemonPort ?? DEFAULT_DAEMON_PORT));
  }, [daemonPort]);

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
            <span>◈</span> Session Settings
          </h2>

          <div className="space-y-6">
            <div>
              <h3 className="font-medium mb-4">Session</h3>
              <div className="space-y-4">
                <div className="flex items-center justify-between p-4 bg-surface2 rounded-lg">
                  <div>
                    <p className="font-medium">Daemon auth state</p>
                    <p className="text-sm text-gray-500">{appState}</p>
                  </div>
                  <button
                    onClick={() => void lock()}
                    className="rounded-lg border border-surface px-4 py-2 text-sm hover:border-primary"
                  >
                    Lock now
                  </button>
                </div>
              </div>
            </div>

            <div>
              <h3 className="font-medium mb-4">Daemon Endpoint</h3>
              <div className="space-y-4">
                <div className="p-4 bg-surface2 rounded-lg space-y-3">
                  <div>
                    <p className="font-medium">Current endpoint</p>
                    <p className="text-sm text-gray-500 break-all">
                      {daemonUrl ?? `Not connected. Default probe port is ${DEFAULT_DAEMON_PORT}.`}
                    </p>
                  </div>
                  <div className="grid gap-3 md:grid-cols-[1fr_auto_auto] md:items-end">
                    <label className="block">
                      <span className="mb-2 block text-sm text-gray-400">Daemon port</span>
                      <input
                        value={portInput}
                        onChange={(event) => setPortInput(event.target.value)}
                        className="w-full rounded-lg border border-surface bg-background px-3 py-3 outline-none focus:border-primary"
                        placeholder={String(DEFAULT_DAEMON_PORT)}
                        inputMode="numeric"
                      />
                    </label>
                    <button
                      onClick={() => void setDaemonPort(portInput.trim() ? Number(portInput) : null)}
                      className="rounded-lg bg-primary px-4 py-3 text-sm hover:bg-primary/80"
                    >
                      Save Port
                    </button>
                    <button
                      onClick={() => {
                        setPortInput(String(DEFAULT_DAEMON_PORT));
                        void setDaemonPort(null);
                      }}
                      className="rounded-lg border border-surface px-4 py-3 text-sm hover:border-primary"
                    >
                      Use Defaults
                    </button>
                  </div>
                  <p className="text-sm text-gray-500">
                    The hosted UI probes the new default port first, then falls back to legacy ports such as 3727 if needed.
                  </p>
                </div>
              </div>
            </div>

            <div>
              <h3 className="font-medium mb-4">Developer Mode</h3>
              <div className="flex items-center justify-between p-4 bg-surface2 rounded-lg">
                <div>
                  <p className="font-medium">Advanced extension installs</p>
                  <p className="text-sm text-gray-500">
                    {config?.developer_mode
                      ? 'Enabled: local package and explicit registry installs are available.'
                      : 'Disabled: only catalog-reviewed installs are exposed in the UI.'}
                  </p>
                </div>
                <button
                  onClick={() => void updateConfig({ developer_mode: !config?.developer_mode })}
                  className={`rounded-lg px-4 py-2 text-sm ${
                    config?.developer_mode
                      ? 'border border-surface hover:border-primary'
                      : 'bg-primary hover:bg-primary/80'
                  }`}
                >
                  {config?.developer_mode ? 'Disable' : 'Enable'}
                </button>
              </div>
            </div>

            <div>
              <h3 className="font-medium mb-4">Daemon Install Modes</h3>
              <div className="space-y-4">
                {serviceInstall ? (
                  ['login', 'boot'].map((mode) => {
                    const state = serviceInstall[mode as 'login' | 'boot'];
                    return (
                      <div key={mode} className="flex items-center justify-between p-4 bg-surface2 rounded-lg">
                        <div>
                          <p className="font-medium">{mode}</p>
                          <p className="text-sm text-gray-500">
                            {state.installed ? 'installed' : 'not installed'} · default profile {state.default_profile}
                          </p>
                          {state.supported ? (
                            <p className="text-sm text-gray-500">{state.path}</p>
                          ) : (
                            <p className="text-sm text-gray-500">Not supported on this platform.</p>
                          )}
                        </div>
                        <div className="flex items-center gap-2">
                          {state.installed ? (
                            <button
                              onClick={() => void uninstallService(mode as 'login' | 'boot')}
                              className="rounded-lg border border-error/30 px-4 py-2 text-sm text-error hover:bg-error/10"
                            >
                              Uninstall
                            </button>
                          ) : (
                            <button
                              onClick={() => void installService(mode as 'login' | 'boot')}
                              disabled={!state.supported}
                              className="rounded-lg bg-primary px-4 py-2 text-sm hover:bg-primary/80 disabled:bg-surface disabled:text-gray-500"
                            >
                              Install
                            </button>
                          )}
                        </div>
                      </div>
                    );
                  })
                ) : (
                  <div className="p-4 bg-surface2 rounded-lg text-sm text-gray-500">Loading service install state…</div>
                )}
              </div>
            </div>

            <div>
              <h3 className="font-medium mb-4">Services</h3>
              <div className="space-y-4">
                {services.map((service) => (
                  <div key={service.name} className="flex items-center justify-between p-4 bg-surface2 rounded-lg">
                    <div>
                      <p className="font-medium">{service.name}</p>
                      <p className="text-sm text-gray-500">
                        {Object.entries(service.details)
                          .map(([key, value]) => `${key}=${value}`)
                          .join(' · ') || 'No extra details'}
                      </p>
                    </div>
                    <button
                      onClick={() => void setServiceEnabled(service.name, !service.enabled)}
                      className={`rounded-lg px-4 py-2 text-sm ${
                        service.enabled
                          ? 'border border-surface hover:border-primary'
                          : 'bg-primary hover:bg-primary/80'
                      }`}
                    >
                      {service.enabled ? 'Disable' : 'Enable'}
                    </button>
                  </div>
                ))}
                <div className="p-4 bg-surface2 rounded-lg">
                  <p className="font-medium mb-2">Node profile</p>
                  <p className="text-sm text-gray-500">
                    {hello?.node.node_name ?? 'unknown'} · {hello?.node.role ?? 'unknown'}
                  </p>
                  <p className="mt-2 text-sm text-gray-500">
                    Idle window: {formatSeconds(authStatus?.idle_expires_in_secs)} · Absolute window: {formatSeconds(authStatus?.absolute_expires_in_secs)}
                  </p>
                </div>
              </div>
            </div>
            <ErrorBanner error={error} onDismiss={clearError} />
          </div>
        </section>
      </main>

      <footer className="border-t border-surface2 py-4 text-center text-sm text-gray-500">
        Rove v0.0.3 — Local-first AI Agent
      </footer>
    </div>
  );
}

function formatSeconds(value: number | null | undefined) {
  if (!value || value <= 0) {
    return 'expired';
  }
  if (value < 60) {
    return `${value}s`;
  }
  const minutes = Math.floor(value / 60);
  const seconds = value % 60;
  return `${minutes}m ${seconds}s`;
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
