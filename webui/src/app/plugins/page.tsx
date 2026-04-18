'use client';

import { useEffect, useState } from 'react';

import Nav from '@/components/Nav';
import { useRoveStore } from '@/stores/roveStore';

type Tab = 'installed' | 'catalog';

export default function PluginsPage() {
  const {
    clearError,
    config,
    error,
    extensionCatalog,
    extensionUpdates,
    extensions,
    initialize,
    installExtension,
    refreshExtensionCatalog,
    refreshExtensionUpdates,
    removeExtension,
    setExtensionEnabled,
    upgradeExtension,
  } = useRoveStore();
  const [tab, setTab] = useState<Tab>('installed');
  const [advancedSource, setAdvancedSource] = useState('');
  const [advancedKind, setAdvancedKind] = useState('');
  const [advancedRegistry, setAdvancedRegistry] = useState('');
  const [advancedVersion, setAdvancedVersion] = useState('');

  useEffect(() => {
    void initialize();
  }, [initialize]);

  return (
    <div className="min-h-screen flex flex-col">
      <header className="sticky top-0 z-10 bg-background/80 backdrop-blur border-b border-surface2">
        <div className="max-w-5xl mx-auto px-4 py-4">
          <div className="flex items-center gap-3 mb-4">
            <span className="text-3xl">🌐</span>
            <h1 className="text-2xl font-bold bg-gradient-to-r from-primary to-purple-500 bg-clip-text text-transparent">
              Rove
            </h1>
          </div>
          <Nav />
        </div>
      </header>

      <main className="flex-1 max-w-5xl w-full mx-auto px-4 py-6 space-y-6">
        <section className="bg-surface rounded-xl p-6 border border-surface2 space-y-6">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <h2 className="text-xl font-semibold flex items-center gap-2">
                <span>▣</span> Extensions
              </h2>
              <p className="mt-2 text-sm text-gray-400">
                Official and verified extensions come from the Rove public catalog. Advanced direct installs are only visible in developer mode.
              </p>
            </div>
            <div className="flex items-center gap-2">
              <button
                onClick={() => void refreshExtensionCatalog(true)}
                className="rounded-lg border border-surface px-4 py-2 text-sm hover:border-primary"
              >
                Refresh Catalog
              </button>
              <button
                onClick={() => void refreshExtensionUpdates()}
                className="rounded-lg border border-surface px-4 py-2 text-sm hover:border-primary"
              >
                Refresh Updates
              </button>
            </div>
          </div>

          <div className="grid gap-4 md:grid-cols-3">
            <StatCard label="Installed" value={String(extensions.length)} />
            <StatCard label="Catalog" value={String(extensionCatalog.length)} />
            <StatCard label="Updates" value={String(extensionUpdates.length)} accent={extensionUpdates.length > 0} />
          </div>

          <div className="flex items-center gap-2">
            <TabButton active={tab === 'installed'} onClick={() => setTab('installed')}>
              Installed
            </TabButton>
            <TabButton active={tab === 'catalog'} onClick={() => setTab('catalog')}>
              Catalog
            </TabButton>
          </div>

          {tab === 'installed' ? (
            <div className="space-y-4">
              {extensions.length === 0 ? (
                <EmptyState text="No installed extensions reported by the local daemon." />
              ) : (
                extensions.map((extension) => (
                  <InstalledExtensionCard
                    key={`${extension.kind}:${extension.id}`}
                    extension={extension}
                    onEnable={() => void setExtensionEnabled(extension.kind, extension.id, true)}
                    onDisable={() => void setExtensionEnabled(extension.kind, extension.id, false)}
                    onRemove={() => void removeExtension(extension.kind, extension.id)}
                    onUpgrade={() =>
                      void upgradeExtension({
                        kind: extension.kind === 'connector' ? undefined : extension.kind,
                        source: extension.id,
                      })
                    }
                  />
                ))
              )}
            </div>
          ) : (
            <div className="space-y-4">
              {extensionCatalog.length === 0 ? (
                <EmptyState text="No public catalog entries are cached yet." />
              ) : (
                extensionCatalog.map((extension) => (
                  <CatalogExtensionCard
                    key={extension.id}
                    extension={extension}
                    onInstall={() =>
                      void installExtension({
                        kind: extension.kind === 'connector' ? undefined : extension.kind,
                        source: extension.id,
                      })
                    }
                    onUpgrade={() =>
                      void upgradeExtension({
                        kind: extension.kind === 'connector' ? undefined : extension.kind,
                        source: extension.id,
                      })
                    }
                  />
                ))
              )}
            </div>
          )}

          {config?.developer_mode ? (
            <section className="rounded-xl border border-warning/30 bg-warning/5 p-5 space-y-4">
              <div>
                <h3 className="font-medium">Advanced Install</h3>
                <p className="mt-2 text-sm text-gray-400">
                  Developer mode is enabled. Use this only for local package directories or explicit registry sources outside the default reviewed catalog.
                </p>
              </div>
              <div className="grid gap-4 md:grid-cols-2">
                <Field label="Source">
                  <input
                    value={advancedSource}
                    onChange={(event) => setAdvancedSource(event.target.value)}
                    className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                    placeholder="/path/to/package or extension-id"
                  />
                </Field>
                <Field label="Kind (optional)">
                  <select
                    value={advancedKind}
                    onChange={(event) => setAdvancedKind(event.target.value)}
                    className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                  >
                    <option value="">Auto</option>
                    <option value="skill">Skill</option>
                    <option value="driver">Driver</option>
                    <option value="channel">Channel</option>
                  </select>
                </Field>
                <Field label="Registry (optional)">
                  <input
                    value={advancedRegistry}
                    onChange={(event) => setAdvancedRegistry(event.target.value)}
                    className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                    placeholder="https://registry.example.com or /path/to/registry"
                  />
                </Field>
                <Field label="Version (optional)">
                  <input
                    value={advancedVersion}
                    onChange={(event) => setAdvancedVersion(event.target.value)}
                    className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                    placeholder="0.1.0"
                  />
                </Field>
              </div>
              <div className="flex flex-wrap gap-2">
                <button
                  onClick={() =>
                    void installExtension({
                      kind: advancedKind || undefined,
                      source: advancedSource,
                      registry: advancedRegistry || undefined,
                      version: advancedVersion || undefined,
                    })
                  }
                  className="rounded-lg bg-primary px-4 py-2 text-sm hover:bg-primary/80"
                >
                  Advanced Install
                </button>
                <button
                  onClick={() =>
                    void upgradeExtension({
                      kind: advancedKind || undefined,
                      source: advancedSource,
                      registry: advancedRegistry || undefined,
                      version: advancedVersion || undefined,
                    })
                  }
                  className="rounded-lg border border-surface px-4 py-2 text-sm hover:border-primary"
                >
                  Advanced Upgrade
                </button>
              </div>
            </section>
          ) : (
            <div className="rounded-lg bg-surface2 p-4 text-sm text-gray-400">
              Advanced local and explicit-registry installs are hidden until developer mode is enabled in Settings or Config.
            </div>
          )}

          <ErrorBanner error={error} onDismiss={clearError} />
        </section>
      </main>

      <footer className="border-t border-surface2 py-4 text-center text-sm text-gray-500">
        Rove v0.0.3 — Local-first AI Agent
      </footer>
    </div>
  );
}

function InstalledExtensionCard({
  extension,
  onEnable,
  onDisable,
  onRemove,
  onUpgrade,
}: {
  extension: {
    id: string;
    name: string;
    kind: string;
    state: string;
    source: string;
    description: string;
    version?: string | null;
    trust_badge: string;
    latest_version?: string | null;
    update_available: boolean;
    provenance: { source: string; registry?: string | null };
    wasm_limits?: {
      timeout_secs: number;
      max_memory_mb: number;
      fuel_limit: number;
      sidecar_path?: string | null;
    } | null;
  };
  onEnable: () => void;
  onDisable: () => void;
  onRemove: () => void;
  onUpgrade: () => void;
}) {
  const isEnabled = extension.state === 'installed';

  return (
    <div className="rounded-xl bg-surface2 p-5 border border-surface space-y-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <p className="font-medium">{extension.name}</p>
            <Badge>{extension.kind}</Badge>
            <TrustBadge badge={extension.trust_badge} />
            <StateBadge state={extension.state} />
          </div>
          <p className="mt-2 text-sm text-gray-400">{extension.description}</p>
        </div>
        <div className="flex flex-wrap gap-2">
          {extension.update_available && (
            <button onClick={onUpgrade} className="rounded-lg bg-primary px-4 py-2 text-sm hover:bg-primary/80">
              Update
            </button>
          )}
          {isEnabled ? (
            <button onClick={onDisable} className="rounded-lg border border-surface px-4 py-2 text-sm hover:border-primary">
              Disable
            </button>
          ) : (
            <button onClick={onEnable} className="rounded-lg bg-primary px-4 py-2 text-sm hover:bg-primary/80">
              Enable
            </button>
          )}
          <button onClick={onRemove} className="rounded-lg border border-error/30 px-4 py-2 text-sm text-error hover:bg-error/10">
            Remove
          </button>
        </div>
      </div>

      <div className="grid gap-3 text-sm text-gray-400 md:grid-cols-2">
        <MetaRow label="Installed version" value={extension.version ?? 'unknown'} />
        <MetaRow label="Latest version" value={extension.latest_version ?? 'unknown'} />
        <MetaRow label="Source" value={extension.provenance.source} />
        <MetaRow label="Registry" value={extension.provenance.registry ?? 'n/a'} />
        {extension.wasm_limits ? (
          <>
            <MetaRow
              label="WASM timeout"
              value={`${extension.wasm_limits.timeout_secs}s`}
            />
            <MetaRow
              label="WASM memory cap"
              value={`${extension.wasm_limits.max_memory_mb} MB`}
            />
            <MetaRow
              label="WASM fuel limit"
              value={String(extension.wasm_limits.fuel_limit)}
            />
            <MetaRow
              label="WASM sidecar"
              value={extension.wasm_limits.sidecar_path ?? 'none'}
            />
          </>
        ) : null}
      </div>
    </div>
  );
}

function CatalogExtensionCard({
  extension,
  onInstall,
  onUpgrade,
}: {
  extension: {
    id: string;
    name: string;
    kind: string;
    description: string;
    trust_badge: string;
    latest: {
      version: string;
      permission_summary: string[];
      permission_warnings: string[];
      release_summary?: string | null;
    };
    installed: boolean;
    installed_version?: string | null;
    update_available: boolean;
  };
  onInstall: () => void;
  onUpgrade: () => void;
}) {
  return (
    <div className="rounded-xl bg-surface2 p-5 border border-surface space-y-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <p className="font-medium">{extension.name}</p>
            <Badge>{extension.kind}</Badge>
            <TrustBadge badge={extension.trust_badge} />
            {extension.installed && <Badge>installed</Badge>}
          </div>
          <p className="mt-2 text-sm text-gray-400">{extension.description}</p>
        </div>
        <div className="flex gap-2">
          {extension.installed && extension.update_available ? (
            <button onClick={onUpgrade} className="rounded-lg bg-primary px-4 py-2 text-sm hover:bg-primary/80">
              Upgrade
            </button>
          ) : (
            <button onClick={onInstall} className="rounded-lg bg-primary px-4 py-2 text-sm hover:bg-primary/80">
              {extension.installed ? 'Reinstall' : 'Install'}
            </button>
          )}
        </div>
      </div>

      <div className="grid gap-3 text-sm text-gray-400 md:grid-cols-2">
        <MetaRow label="Latest version" value={extension.latest.version} />
        <MetaRow label="Installed version" value={extension.installed_version ?? 'not installed'} />
      </div>

      <div className="space-y-2 text-sm">
        <p className="text-gray-300">Permissions</p>
        <ul className="space-y-1 text-gray-400">
          {extension.latest.permission_summary.map((line) => (
            <li key={line}>• {line}</li>
          ))}
        </ul>
        {extension.latest.permission_warnings.length > 0 && (
          <div className="rounded-lg border border-warning/30 bg-warning/10 p-3 text-warning">
            {extension.latest.permission_warnings.join(' · ')}
          </div>
        )}
        {extension.latest.release_summary && (
          <p className="text-gray-500">{extension.latest.release_summary}</p>
        )}
      </div>
    </div>
  );
}

function StatCard({ label, value, accent = false }: { label: string; value: string; accent?: boolean }) {
  return (
    <div className={`rounded-xl border p-4 ${accent ? 'border-primary/40 bg-primary/10' : 'border-surface bg-surface2'}`}>
      <p className="text-sm text-gray-400">{label}</p>
      <p className="mt-2 text-2xl font-semibold">{value}</p>
    </div>
  );
}

function TabButton({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      className={`rounded-lg px-4 py-2 text-sm ${active ? 'bg-primary text-white' : 'border border-surface text-gray-300 hover:border-primary'}`}
    >
      {children}
    </button>
  );
}

function Badge({ children }: { children: React.ReactNode }) {
  return <span className="rounded-full border border-surface px-2 py-0.5 text-xs uppercase text-gray-400">{children}</span>;
}

function TrustBadge({ badge }: { badge: string }) {
  const tone =
    badge === 'official'
      ? 'border-success/30 text-success'
      : badge === 'verified'
        ? 'border-primary/30 text-primary'
        : 'border-warning/30 text-warning';
  return <span className={`rounded-full border px-2 py-0.5 text-xs uppercase ${tone}`}>{badge}</span>;
}

function StateBadge({ state }: { state: string }) {
  const tone =
    state === 'installed'
      ? 'bg-success/20 text-success'
      : state === 'installed-disabled'
        ? 'bg-warning/20 text-warning'
        : 'bg-surface text-gray-400';
  return <span className={`rounded px-2 py-0.5 text-xs ${tone}`}>{state}</span>;
}

function MetaRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg bg-background px-3 py-2">
      <p className="text-xs uppercase tracking-wide text-gray-500">{label}</p>
      <p className="mt-1 break-all text-gray-300">{value}</p>
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

function EmptyState({ text }: { text: string }) {
  return <div className="rounded-lg bg-surface2 p-4 text-sm text-gray-400">{text}</div>;
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
