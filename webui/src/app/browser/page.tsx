'use client';

import { useEffect, useState } from 'react';

import Nav from '@/components/Nav';
import {
  BrowserApprovalControls,
  BrowserProfileInput,
  BrowserProfileMode,
  BrowserProfileRecord,
  BrowserSurfaceStatus,
  DaemonError,
  RoveDaemonClient,
  readStoredToken,
} from '@/lib/daemon';

const DEFAULT_CONTROLS: BrowserApprovalControls = {
  require_approval_for_managed_launch: true,
  require_approval_for_existing_session_attach: true,
  require_approval_for_remote_cdp: true,
};

const EMPTY_PROFILE: ProfileDraft = {
  id: '',
  name: '',
  enabled: true,
  mode: 'managed_local',
  browser: '',
  user_data_dir: '',
  startup_url: '',
  cdp_url: '',
  notes: '',
};

type ProfileDraft = {
  id: string;
  name: string;
  enabled: boolean;
  mode: BrowserProfileMode;
  browser: string;
  user_data_dir: string;
  startup_url: string;
  cdp_url: string;
  notes: string;
};

export default function BrowserPage() {
  const [surface, setSurface] = useState<BrowserSurfaceStatus | null>(null);
  const [enabled, setEnabled] = useState(false);
  const [controls, setControls] = useState<BrowserApprovalControls>(DEFAULT_CONTROLS);
  const [profiles, setProfiles] = useState<BrowserProfileInput[]>([]);
  const [defaultProfileId, setDefaultProfileId] = useState('');
  const [draft, setDraft] = useState<ProfileDraft>(EMPTY_PROFILE);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const displayProfiles = profiles.map((profile) =>
    toDisplayProfile(
      profile,
      surface?.profiles.find((savedProfile) => savedProfile.id === profile.id) ?? null,
      defaultProfileId,
      controls,
    ),
  );

  useEffect(() => {
    void refresh();
  }, []);

  async function refresh() {
    setLoading(true);
    setError(null);
    try {
      const nextSurface = await daemonClient().getBrowserSurface();
      applySurface(nextSurface);
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setLoading(false);
    }
  }

  function applySurface(nextSurface: BrowserSurfaceStatus) {
    setSurface(nextSurface);
    setEnabled(nextSurface.enabled);
    setControls(nextSurface.controls);
    setProfiles(nextSurface.profiles.map(profileToInput));
    setDefaultProfileId(nextSurface.default_profile_id ?? '');
    resetDraft();
  }

  function resetDraft() {
    setDraft(EMPTY_PROFILE);
    setEditingId(null);
  }

  function editProfile(profile: BrowserProfileRecord) {
    setDraft({
      id: profile.id,
      name: profile.name,
      enabled: profile.enabled,
      mode: profile.mode,
      browser: profile.browser ?? '',
      user_data_dir: profile.user_data_dir ?? '',
      startup_url: profile.startup_url ?? '',
      cdp_url: profile.cdp_url ?? '',
      notes: profile.notes ?? '',
    });
    setEditingId(profile.id);
  }

  function saveDraftProfile() {
    if (!draft.id.trim() || !draft.name.trim()) {
      setError('Browser profiles need both an id and a name.');
      return;
    }

    const nextProfile = draftToInput(draft);
    setProfiles((current) => {
      const next = current.filter((profile) => profile.id !== editingId && profile.id !== nextProfile.id);
      next.push(nextProfile);
      next.sort((left, right) => left.name.localeCompare(right.name));
      return next;
    });
    if (editingId && defaultProfileId === editingId) {
      setDefaultProfileId(nextProfile.id);
    } else if (!defaultProfileId) {
      setDefaultProfileId(nextProfile.id);
    }
    setError(null);
    resetDraft();
  }

  function removeProfile(id: string) {
    setProfiles((current) => {
      const next = current.filter((profile) => profile.id !== id);
      if (defaultProfileId === id) {
        setDefaultProfileId(next[0]?.id ?? '');
      }
      return next;
    });
    if (editingId === id) {
      resetDraft();
    }
  }

  async function saveSurface() {
    setSaving(true);
    setError(null);
    try {
      const nextSurface = await daemonClient().updateBrowserSurface({
        enabled,
        default_profile_id: defaultProfileId || null,
        controls,
        profiles,
      });
      applySurface(nextSurface);
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="min-h-screen flex flex-col">
      <header className="sticky top-0 z-10 bg-background/80 backdrop-blur border-b border-surface2">
        <div className="max-w-6xl mx-auto px-4 py-4 space-y-4">
          <div>
            <h1 className="text-2xl font-semibold">Browser Control</h1>
            <p className="text-sm text-gray-400">
              Controlled browser profiles for daemon-native automation. This surface defines trust boundaries and approval requirements first; deeper browser execution can build on top of these profiles later.
            </p>
          </div>
          <Nav />
        </div>
      </header>

      <main className="flex-1 max-w-6xl w-full mx-auto px-4 py-6 space-y-6">
        <section className="grid gap-4 md:grid-cols-4">
          <StatCard label="Surface" value={enabled ? 'enabled' : 'disabled'} />
          <StatCard label="Profiles" value={String(profiles.length)} />
          <StatCard label="Default" value={defaultProfileId || 'not selected'} />
          <StatCard
            label="High-trust modes"
            value={`${profiles.filter((profile) => profile.mode !== 'managed_local').length} warning profiles`}
          />
        </section>

        {error ? (
          <section className="rounded-xl border border-red-500/40 bg-red-500/10 px-4 py-3 text-sm text-red-200">
            {error}
          </section>
        ) : null}

        {loading ? (
          <section className="rounded-xl border border-surface2 bg-surface px-4 py-6 text-sm text-gray-400">
            Loading browser surface...
          </section>
        ) : null}

        {!loading ? (
          <>
            <section className="rounded-xl border border-surface2 bg-surface p-6 space-y-5">
              <div className="flex flex-wrap items-start justify-between gap-4">
                <div className="space-y-2">
                  <h2 className="text-lg font-semibold">Operator Controls</h2>
                  <p className="text-sm text-gray-400 max-w-3xl">
                    Managed-local profiles keep browser lifecycle under the daemon. Attach-existing and remote-CDP profiles are still official, but they are explicitly higher-trust modes and should stay approval-gated.
                  </p>
                </div>
                <div className="flex items-center gap-3">
                  <button
                    onClick={() => void refresh()}
                    className="rounded-lg border border-surface2 px-4 py-2 text-sm hover:border-primary"
                  >
                    Refresh
                  </button>
                  <button
                    onClick={() => void saveSurface()}
                    disabled={saving}
                    className="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-white hover:bg-primary/80 disabled:opacity-60"
                  >
                    {saving ? 'Saving...' : 'Save Browser Surface'}
                  </button>
                </div>
              </div>

              <label className="flex items-center gap-3 rounded-lg border border-surface2 bg-background/50 px-4 py-3 text-sm text-gray-200">
                <input
                  type="checkbox"
                  checked={enabled}
                  onChange={(event) => setEnabled(event.target.checked)}
                />
                Enable the browser control surface
              </label>

              <div className="grid gap-4 md:grid-cols-3">
                <CheckboxCard
                  label="Managed launch approval"
                  checked={controls.require_approval_for_managed_launch}
                  onChange={(checked) =>
                    setControls((current) => ({
                      ...current,
                      require_approval_for_managed_launch: checked,
                    }))
                  }
                  note="Require approval before daemon-managed browser launches."
                />
                <CheckboxCard
                  label="Existing-session approval"
                  checked={controls.require_approval_for_existing_session_attach}
                  onChange={(checked) =>
                    setControls((current) => ({
                      ...current,
                      require_approval_for_existing_session_attach: checked,
                    }))
                  }
                  note="Keep attach-existing profiles behind approval because they inherit live cookies and tabs."
                />
                <CheckboxCard
                  label="Remote CDP approval"
                  checked={controls.require_approval_for_remote_cdp}
                  onChange={(checked) =>
                    setControls((current) => ({
                      ...current,
                      require_approval_for_remote_cdp: checked,
                    }))
                  }
                  note="Remote CDP crosses the local trust boundary and should usually stay gated."
                />
              </div>

              <label className="block space-y-2 text-sm">
                <span className="text-gray-400">Default profile</span>
                <select
                  value={defaultProfileId}
                  onChange={(event) => setDefaultProfileId(event.target.value)}
                  className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                >
                  <option value="">Select a default profile</option>
                  {profiles.map((profile) => (
                    <option key={profile.id} value={profile.id}>
                      {profile.name} ({profile.id})
                    </option>
                  ))}
                </select>
              </label>

              {surface?.warnings.length ? (
                <div className="rounded-xl border border-amber-500/30 bg-amber-500/10 px-4 py-4">
                  <p className="text-sm font-medium text-amber-100">Current operator warnings</p>
                  <ul className="mt-2 space-y-1 text-sm text-amber-50/90">
                    {surface.warnings.map((warning) => (
                      <li key={warning}>- {warning}</li>
                    ))}
                  </ul>
                </div>
              ) : null}
            </section>

            <section className="grid gap-6 xl:grid-cols-[1.2fr_0.8fr]">
              <section className="rounded-xl border border-surface2 bg-surface p-6 space-y-4">
                <div>
                  <h2 className="text-lg font-semibold">Profiles</h2>
                  <p className="text-sm text-gray-400">
                    Keep profile modes explicit. `managed_local` is the safest default. `attach_existing` and `remote_cdp` are intentionally noisier because they inherit or cross trust boundaries.
                  </p>
                </div>

                {displayProfiles.length ? (
                  <div className="space-y-3">
                    {displayProfiles.map((profile) => (
                      <article key={profile.id} className="rounded-xl border border-surface2 bg-background/50 p-4 space-y-3">
                        <div className="flex flex-wrap items-start justify-between gap-3">
                          <div className="space-y-2">
                            <div className="flex flex-wrap items-center gap-2">
                              <h3 className="text-base font-semibold">{profile.name}</h3>
                              <Badge>{profile.mode.replace('_', ' ')}</Badge>
                              <Badge tone={profile.readiness === 'ready' ? 'success' : 'warning'}>
                                {profile.readiness.replace('_', ' ')}
                              </Badge>
                              {profile.is_default ? <Badge tone="primary">default</Badge> : null}
                              {!profile.enabled ? <Badge>disabled</Badge> : null}
                            </div>
                            <p className="text-sm text-gray-400">{profile.id}</p>
                          </div>
                          <div className="flex items-center gap-2">
                            <button
                              onClick={() => editProfile(profile)}
                              className="rounded-lg border border-surface2 px-3 py-2 text-sm hover:border-primary"
                            >
                              Edit
                            </button>
                            <button
                              onClick={() => removeProfile(profile.id)}
                              className="rounded-lg border border-red-500/30 px-3 py-2 text-sm text-red-200 hover:bg-red-500/10"
                            >
                              Remove
                            </button>
                          </div>
                        </div>

                        <div className="grid gap-3 md:grid-cols-2 text-sm text-gray-300">
                          <Detail label="Approval" value={profile.approval_required ? 'required' : 'not required'} />
                          <Detail label="Browser" value={profile.browser ?? 'not specified'} />
                          <Detail label="User data" value={profile.user_data_dir ?? 'not specified'} />
                          <Detail label="Startup URL" value={profile.startup_url ?? 'not specified'} />
                          <Detail label="CDP endpoint" value={profile.cdp_url ?? 'not specified'} />
                        </div>

                        {profile.notes ? <p className="text-sm text-gray-400">{profile.notes}</p> : null}

                        {profile.warnings.length ? (
                          <ul className="space-y-1 text-sm text-amber-100">
                            {profile.warnings.map((warning) => (
                              <li key={warning}>- {warning}</li>
                            ))}
                          </ul>
                        ) : null}
                      </article>
                    ))}
                  </div>
                ) : (
                  <p className="rounded-xl border border-surface2 bg-background/50 px-4 py-5 text-sm text-gray-400">
                    No browser profiles configured yet.
                  </p>
                )}
              </section>

              <section className="rounded-xl border border-surface2 bg-surface p-6 space-y-4">
                <div>
                  <h2 className="text-lg font-semibold">{editingId ? 'Edit Profile' : 'Add Profile'}</h2>
                  <p className="text-sm text-gray-400">
                    Save the draft locally first, then persist the full browser surface with “Save Browser Surface”.
                  </p>
                </div>

                <Field label="Profile id">
                  <input
                    value={draft.id}
                    onChange={(event) => setDraft((current) => ({ ...current, id: event.target.value }))}
                    className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                    placeholder="ops-browser"
                  />
                </Field>

                <Field label="Name">
                  <input
                    value={draft.name}
                    onChange={(event) => setDraft((current) => ({ ...current, name: event.target.value }))}
                    className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                    placeholder="Ops Browser"
                  />
                </Field>

                <Field label="Mode">
                  <select
                    value={draft.mode}
                    onChange={(event) =>
                      setDraft((current) => ({
                        ...current,
                        mode: event.target.value as BrowserProfileMode,
                      }))
                    }
                    className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                  >
                    <option value="managed_local">managed_local</option>
                    <option value="attach_existing">attach_existing</option>
                    <option value="remote_cdp">remote_cdp</option>
                  </select>
                </Field>

                <label className="flex items-center gap-3 text-sm text-gray-200">
                  <input
                    type="checkbox"
                    checked={draft.enabled}
                    onChange={(event) =>
                      setDraft((current) => ({ ...current, enabled: event.target.checked }))
                    }
                  />
                  Profile enabled
                </label>

                <Field label="Browser hint">
                  <input
                    value={draft.browser}
                    onChange={(event) => setDraft((current) => ({ ...current, browser: event.target.value }))}
                    className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                    placeholder="chrome | chromium | edge | brave"
                  />
                </Field>

                <Field label="User data dir">
                  <input
                    value={draft.user_data_dir}
                    onChange={(event) => setDraft((current) => ({ ...current, user_data_dir: event.target.value }))}
                    className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                    placeholder="~/Library/Application Support/Rove/browser"
                  />
                </Field>

                <Field label="Startup URL">
                  <input
                    value={draft.startup_url}
                    onChange={(event) => setDraft((current) => ({ ...current, startup_url: event.target.value }))}
                    className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                    placeholder="https://app.roveai.co"
                  />
                </Field>

                <Field label={draft.mode === 'managed_local' ? 'CDP endpoint (optional)' : 'CDP endpoint (required)'}>
                  <input
                    value={draft.cdp_url}
                    onChange={(event) => setDraft((current) => ({ ...current, cdp_url: event.target.value }))}
                    className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                    placeholder={draft.mode === 'remote_cdp' ? 'wss://browser.example/ws' : 'http://127.0.0.1:9222'}
                  />
                </Field>

                <Field label="Notes">
                  <textarea
                    value={draft.notes}
                    onChange={(event) => setDraft((current) => ({ ...current, notes: event.target.value }))}
                    className="min-h-28 w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                    placeholder="Use attach_existing only for explicitly trusted local sessions."
                  />
                </Field>

                <div className="flex items-center gap-3">
                  <button
                    onClick={saveDraftProfile}
                    className="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-white hover:bg-primary/80"
                  >
                    {editingId ? 'Update Draft' : 'Add Draft'}
                  </button>
                  <button
                    onClick={resetDraft}
                    className="rounded-lg border border-surface2 px-4 py-2 text-sm hover:border-primary"
                  >
                    Reset
                  </button>
                </div>
              </section>
            </section>
          </>
        ) : null}
      </main>
    </div>
  );
}

function profileToInput(profile: BrowserProfileRecord): BrowserProfileInput {
  return {
    id: profile.id,
    name: profile.name,
    enabled: profile.enabled,
    mode: profile.mode,
    browser: profile.browser,
    user_data_dir: profile.user_data_dir,
    startup_url: profile.startup_url,
    cdp_url: profile.cdp_url,
    notes: profile.notes,
  };
}

function toDisplayProfile(
  profile: BrowserProfileInput,
  savedProfile: BrowserProfileRecord | null,
  defaultProfileId: string,
  controls: BrowserApprovalControls,
): BrowserProfileRecord {
  if (savedProfile) {
    return {
      ...savedProfile,
      ...profile,
      is_default: defaultProfileId === profile.id,
      approval_required: approvalRequired(profile.mode, controls),
    };
  }

  const warnings: string[] = [];
  let readiness: BrowserProfileRecord['readiness'] = 'ready';
  if (profile.mode === 'attach_existing') {
    readiness = profile.cdp_url ? 'warning' : 'needs_setup';
    warnings.push(
      'Attaches to an already-running browser session. Existing tabs, cookies, and signed-in state are in scope.',
    );
  }
  if (profile.mode === 'remote_cdp') {
    readiness = profile.cdp_url ? 'warning' : 'needs_setup';
    warnings.push(
      'Uses a remote CDP endpoint outside the local node boundary. Treat the browser host and transport path as part of the trust model.',
    );
  }
  if ((profile.mode === 'attach_existing' || profile.mode === 'remote_cdp') && !profile.cdp_url) {
    warnings.push('This profile needs a CDP endpoint before it can be used.');
  }

  return {
    ...profile,
    browser: profile.browser ?? null,
    user_data_dir: profile.user_data_dir ?? null,
    startup_url: profile.startup_url ?? null,
    cdp_url: profile.cdp_url ?? null,
    notes: profile.notes ?? null,
    is_default: defaultProfileId === profile.id,
    readiness,
    approval_required: approvalRequired(profile.mode, controls),
    warnings,
  };
}

function draftToInput(draft: ProfileDraft): BrowserProfileInput {
  return {
    id: draft.id.trim(),
    name: draft.name.trim(),
    enabled: draft.enabled,
    mode: draft.mode,
    browser: normalizeOptionalString(draft.browser),
    user_data_dir: normalizeOptionalString(draft.user_data_dir),
    startup_url: normalizeOptionalString(draft.startup_url),
    cdp_url: normalizeOptionalString(draft.cdp_url),
    notes: normalizeOptionalString(draft.notes),
  };
}

function normalizeOptionalString(value: string): string | null {
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

function approvalRequired(mode: BrowserProfileMode, controls: BrowserApprovalControls): boolean {
  switch (mode) {
    case 'managed_local':
      return controls.require_approval_for_managed_launch;
    case 'attach_existing':
      return controls.require_approval_for_existing_session_attach;
    case 'remote_cdp':
      return controls.require_approval_for_remote_cdp;
  }
}

function daemonClient() {
  const token = readStoredToken();
  if (!token) {
    throw new Error('Missing daemon token.');
  }
  return new RoveDaemonClient(token);
}

function formatError(error: unknown): string {
  if (error instanceof DaemonError) {
    return error.message;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return 'Unknown error';
}

function StatCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl border border-surface2 bg-surface px-4 py-4">
      <p className="text-sm text-gray-500">{label}</p>
      <p className="mt-2 text-lg font-semibold text-gray-100">{value}</p>
    </div>
  );
}

function CheckboxCard({
  label,
  note,
  checked,
  onChange,
}: {
  label: string;
  note: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="rounded-xl border border-surface2 bg-background/50 px-4 py-4 space-y-3 text-sm text-gray-200">
      <div className="flex items-center gap-3">
        <input type="checkbox" checked={checked} onChange={(event) => onChange(event.target.checked)} />
        <span className="font-medium">{label}</span>
      </div>
      <p className="text-gray-400">{note}</p>
    </label>
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

function Detail({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-surface2 bg-background/40 px-3 py-2">
      <p className="text-xs uppercase tracking-[0.2em] text-gray-500">{label}</p>
      <p className="mt-1 break-all text-sm text-gray-200">{value}</p>
    </div>
  );
}

function Badge({
  children,
  tone = 'default',
}: {
  children: React.ReactNode;
  tone?: 'default' | 'primary' | 'success' | 'warning';
}) {
  const className =
    tone === 'primary'
      ? 'border-primary/30 bg-primary/10 text-primary'
      : tone === 'success'
        ? 'border-emerald-500/30 bg-emerald-500/10 text-emerald-200'
        : tone === 'warning'
          ? 'border-amber-500/30 bg-amber-500/10 text-amber-100'
          : 'border-surface2 bg-background/50 text-gray-300';

  return <span className={`rounded-full border px-2 py-0.5 text-xs ${className}`}>{children}</span>;
}
