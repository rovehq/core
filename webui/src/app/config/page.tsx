'use client';

import { useEffect, useState } from 'react';

import Nav from '@/components/Nav';
import { useRoveStore } from '@/stores/roveStore';

export default function ConfigPage() {
  const { appState, clearError, config, daemonUrl, error, hello, initialize, remoteStatus, updateConfig } = useRoveStore();
  const [nodeName, setNodeName] = useState('');
  const [profile, setProfile] = useState<'desktop' | 'headless'>('desktop');
  const [developerMode, setDeveloperMode] = useState(false);
  const [privacyMode, setPrivacyMode] = useState('local_only');
  const [idleTimeout, setIdleTimeout] = useState('1200');
  const [absoluteTimeout, setAbsoluteTimeout] = useState('43200');
  const [reauthWindow, setReauthWindow] = useState('600');
  const [persistOnRestart, setPersistOnRestart] = useState(false);
  const [approvalMode, setApprovalMode] = useState<'default' | 'allowlist' | 'open' | 'assisted'>('default');
  const [secretBackend, setSecretBackend] = useState<'auto' | 'vault' | 'keychain' | 'env'>('auto');

  useEffect(() => {
    void initialize();
  }, [initialize]);

  useEffect(() => {
    if (!config) {
      return;
    }
    setNodeName(config.node_name);
    setProfile(config.profile);
    setDeveloperMode(config.developer_mode);
    setPrivacyMode(config.privacy_mode);
    setIdleTimeout(String(config.idle_timeout_secs));
    setAbsoluteTimeout(String(config.absolute_timeout_secs));
    setReauthWindow(String(config.reauth_window_secs));
    setPersistOnRestart(config.session_persist_on_restart);
    setApprovalMode(config.approval_mode);
    setSecretBackend(config.secret_backend);
  }, [config]);

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
                <SettingRow label="Node ID" value={remoteStatus?.node.node_id ?? 'unknown'} />
                <SettingRow label="Public key" value={remoteStatus?.node.public_key ?? 'unknown'} />
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

            <form
              className="p-4 bg-surface2 rounded-lg space-y-4"
              onSubmit={async (event) => {
                event.preventDefault();
                await updateConfig({
                  node_name: nodeName,
                  profile,
                  developer_mode: developerMode,
                  privacy_mode: privacyMode,
                  idle_timeout_secs: Number(idleTimeout),
                  absolute_timeout_secs: Number(absoluteTimeout),
                  reauth_window_secs: Number(reauthWindow),
                  session_persist_on_restart: persistOnRestart,
                  approval_mode: approvalMode,
                  secret_backend: secretBackend,
                });
              }}
            >
              <h3 className="font-medium">Daemon Settings</h3>
              <Field label="Profile">
                <select
                  value={profile}
                  onChange={(event) => setProfile(event.target.value as 'desktop' | 'headless')}
                  className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                >
                  <option value="desktop">Desktop</option>
                  <option value="headless">Headless</option>
                </select>
              </Field>
              <Field label="Node name">
                <input
                  value={nodeName}
                  onChange={(event) => setNodeName(event.target.value)}
                  className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                />
              </Field>
              <label className="flex items-center gap-3 text-sm text-gray-300">
                <input
                  type="checkbox"
                  checked={developerMode}
                  onChange={(event) => setDeveloperMode(event.target.checked)}
                />
                Enable developer mode for advanced local/registry extension installs
              </label>
              <Field label="Privacy mode">
                <select
                  value={privacyMode}
                  onChange={(event) => setPrivacyMode(event.target.value)}
                  className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                >
                  <option value="local_only">Local only</option>
                  <option value="hybrid">Hybrid</option>
                  <option value="cloud_enabled">Cloud enabled</option>
                </select>
              </Field>
              <div className="grid gap-4 md:grid-cols-2">
                <Field label="Approval mode">
                  <select
                    value={approvalMode}
                    onChange={(event) => setApprovalMode(event.target.value as 'default' | 'allowlist' | 'open' | 'assisted')}
                    className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                  >
                    <option value="default">Default</option>
                    <option value="allowlist">Allowlist</option>
                    <option value="open">Open</option>
                    <option value="assisted">Assisted</option>
                  </select>
                </Field>
                <Field label="Secret backend">
                  <select
                    value={secretBackend}
                    onChange={(event) => setSecretBackend(event.target.value as 'auto' | 'vault' | 'keychain' | 'env')}
                    className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                  >
                    <option value="auto">Auto</option>
                    <option value="vault">Vault</option>
                    <option value="keychain">Keychain</option>
                    <option value="env">Env</option>
                  </select>
                </Field>
              </div>
              <div className="grid gap-4 md:grid-cols-3">
                <Field label="Idle timeout (secs)">
                  <input value={idleTimeout} onChange={(event) => setIdleTimeout(event.target.value)} className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary" />
                </Field>
                <Field label="Absolute timeout (secs)">
                  <input value={absoluteTimeout} onChange={(event) => setAbsoluteTimeout(event.target.value)} className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary" />
                </Field>
                <Field label="Reauth window (secs)">
                  <input value={reauthWindow} onChange={(event) => setReauthWindow(event.target.value)} className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary" />
                </Field>
              </div>
              <label className="flex items-center gap-3 text-sm text-gray-300">
                <input
                  type="checkbox"
                  checked={persistOnRestart}
                  onChange={(event) => setPersistOnRestart(event.target.checked)}
                />
                Keep sessions after daemon restart
              </label>
              <div className="rounded-lg bg-background p-3 text-sm text-gray-400">
                Approval rules: <code>{config?.approvals_rules_path ?? 'unknown'}</code><br />
                TLS: {config?.tls_enabled ? 'enabled' : 'disabled'}<br />
                Cert: <code>{config?.tls_cert_path ?? 'unknown'}</code><br />
                Key: <code>{config?.tls_key_path ?? 'unknown'}</code>
              </div>
              <ErrorBanner error={error} onDismiss={clearError} />
              <button className="rounded-lg bg-primary px-4 py-2 font-medium hover:bg-primary/80">
                Save Config
              </button>
            </form>
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

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="block space-y-2 text-sm">
      <span className="text-gray-400">{label}</span>
      {children}
    </label>
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
