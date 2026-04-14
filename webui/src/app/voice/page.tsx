'use client';

import { useEffect, useState } from 'react';

import Nav from '@/components/Nav';
import {
  DaemonError,
  RoveDaemonClient,
  VoiceEngineInput,
  VoiceEngineKind,
  VoiceEngineRecord,
  VoiceOutputTestRequest,
  VoicePolicyControls,
  VoiceSurfaceStatus,
  readStoredToken,
} from '@/lib/daemon';

const DEFAULT_POLICY: VoicePolicyControls = {
  require_approval_for_tts: true,
  require_approval_for_stt: true,
  allow_remote_audio_input: false,
  allow_remote_audio_output: false,
  persist_transcripts: false,
};

type EngineDraft = {
  model: string;
  voice: string;
  runtime_path: string;
  notes: string;
  enabled: boolean;
};

const EMPTY_DRAFT: EngineDraft = {
  model: '',
  voice: '',
  runtime_path: '',
  notes: '',
  enabled: true,
};

export default function VoicePage() {
  const [surface, setSurface] = useState<VoiceSurfaceStatus | null>(null);
  const [enabled, setEnabled] = useState(false);
  const [policy, setPolicy] = useState<VoicePolicyControls>(DEFAULT_POLICY);
  const [activeInputEngine, setActiveInputEngine] = useState<VoiceEngineKind | ''>('');
  const [activeOutputEngine, setActiveOutputEngine] = useState<VoiceEngineKind | ''>('');
  const [selectedInputDeviceId, setSelectedInputDeviceId] = useState('');
  const [selectedOutputDeviceId, setSelectedOutputDeviceId] = useState('');
  const [drafts, setDrafts] = useState<Record<VoiceEngineKind, EngineDraft>>(emptyDrafts());
  const [testText, setTestText] = useState('Rove voice output check');
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    void refresh();
  }, []);

  async function refresh() {
    setLoading(true);
    setError(null);
    try {
      const nextSurface = await daemonClient().getVoiceSurface();
      applySurface(nextSurface);
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setLoading(false);
    }
  }

  function applySurface(nextSurface: VoiceSurfaceStatus) {
    setSurface(nextSurface);
    setEnabled(nextSurface.enabled);
    setPolicy(nextSurface.policy);
    setActiveInputEngine(nextSurface.active_input_engine ?? '');
    setActiveOutputEngine(nextSurface.active_output_engine ?? '');
    setSelectedInputDeviceId(nextSurface.selected_input_device_id ?? '');
    setSelectedOutputDeviceId(nextSurface.selected_output_device_id ?? '');
    setDrafts(buildDrafts(nextSurface.engines));
  }

  function updateDraft(kind: VoiceEngineKind, patch: Partial<EngineDraft>) {
    setDrafts((current) => ({
      ...current,
      [kind]: {
        ...current[kind],
        ...patch,
      },
    }));
  }

  async function saveSurface() {
    setSaving(true);
    setError(null);
    setMessage(null);
    try {
      if (enabled && !surface?.runtime.installed) {
        await daemonClient().installVoiceEngine({ engine: 'native_os' });
      }

      const nextSurface = await daemonClient().updateVoiceSurface({
        enabled,
        active_input_engine: activeInputEngine || null,
        active_output_engine: activeOutputEngine || null,
        selected_input_device_id: selectedInputDeviceId || null,
        selected_output_device_id: selectedOutputDeviceId || null,
        policy,
        engines: configuredEngineInputs(surface?.engines ?? [], drafts),
      });
      applySurface(nextSurface);
      setMessage('Voice surface saved.');
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setSaving(false);
    }
  }

  async function installEngine(kind: VoiceEngineKind) {
    setSaving(true);
    setError(null);
    setMessage(null);
    try {
      const draft = drafts[kind];
      const nextSurface = await daemonClient().installVoiceEngine({
        engine: kind,
        model: draft.model || null,
        voice: draft.voice || null,
        runtime_path: draft.runtime_path || null,
        notes: draft.notes || null,
      });
      applySurface(nextSurface);
      setMessage(`Installed ${kind}.`);
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setSaving(false);
    }
  }

  async function uninstallEngine(kind: VoiceEngineKind) {
    setSaving(true);
    setError(null);
    setMessage(null);
    try {
      const nextSurface = await daemonClient().uninstallVoiceEngine({ engine: kind });
      applySurface(nextSurface);
      setMessage(`Removed ${kind}.`);
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setSaving(false);
    }
  }

  async function activateInput(kind: VoiceEngineKind) {
    setSaving(true);
    setError(null);
    setMessage(null);
    try {
      const nextSurface = await daemonClient().activateVoiceInput({ engine: kind });
      applySurface(nextSurface);
      setMessage(`Activated ${kind} for input.`);
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setSaving(false);
    }
  }

  async function activateOutput(kind: VoiceEngineKind) {
    setSaving(true);
    setError(null);
    setMessage(null);
    try {
      const nextSurface = await daemonClient().activateVoiceOutput({ engine: kind });
      applySurface(nextSurface);
      setMessage(`Activated ${kind} for output.`);
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setSaving(false);
    }
  }

  async function testInput() {
    setSaving(true);
    setError(null);
    setMessage(null);
    try {
      const result = await daemonClient().testVoiceInput();
      setMessage(result.message);
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setSaving(false);
    }
  }

  async function testOutput() {
    setSaving(true);
    setError(null);
    setMessage(null);
    try {
      const payload: VoiceOutputTestRequest = {
        text: testText,
        voice: drafts.local_piper.voice || drafts.native_os.voice || null,
      };
      const result = await daemonClient().testVoiceOutput(payload);
      setMessage(result.message);
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
            <h1 className="text-2xl font-semibold">Voice Pack</h1>
            <p className="text-sm text-gray-400">
              Voice is optional. Install the official native runtime only when you want local audio
              devices, then layer native or self-hosted engines on demand.
            </p>
          </div>
          <Nav />
        </div>
      </header>

      <main className="flex-1 max-w-6xl w-full mx-auto px-4 py-6 space-y-6">
        <section className="grid gap-4 md:grid-cols-4">
          <StatCard label="Surface" value={enabled ? 'enabled' : 'disabled'} />
          <StatCard
            label="Voice Pack"
            value={
              surface?.runtime.installed
                ? surface.runtime.enabled
                  ? 'installed'
                  : 'disabled'
                : 'not installed'
            }
          />
          <StatCard label="Input Engine" value={activeInputEngine || 'not selected'} />
          <StatCard label="Output Engine" value={activeOutputEngine || 'not selected'} />
        </section>

        {error ? (
          <section className="rounded-xl border border-red-500/40 bg-red-500/10 px-4 py-3 text-sm text-red-200">
            {error}
          </section>
        ) : null}

        {message ? (
          <section className="rounded-xl border border-emerald-500/40 bg-emerald-500/10 px-4 py-3 text-sm text-emerald-200">
            {message}
          </section>
        ) : null}

        {loading ? (
          <section className="rounded-xl border border-surface2 bg-surface px-4 py-6 text-sm text-gray-400">
            Loading voice surface...
          </section>
        ) : null}

        {!loading && surface ? (
          <>
            <section className="rounded-xl border border-surface2 bg-surface p-6 space-y-5">
              <div className="flex flex-wrap items-start justify-between gap-4">
                <div className="space-y-2">
                  <h2 className="text-lg font-semibold">Operator Controls</h2>
                  <p className="text-sm text-gray-400 max-w-3xl">
                    Core owns policy, routing, and activation. Engines stay optional and should not
                    appear unless the operator installs them.
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
                    {saving ? 'Saving...' : 'Save Voice Surface'}
                  </button>
                </div>
              </div>

              <label className="flex items-center gap-3 rounded-lg border border-surface2 bg-background/50 px-4 py-3 text-sm text-gray-200">
                <input
                  type="checkbox"
                  checked={enabled}
                  onChange={(event) => setEnabled(event.target.checked)}
                />
                Enable voice support
              </label>

              <div className="grid gap-4 md:grid-cols-2">
                <SelectField
                  label="Active Input Engine"
                  value={activeInputEngine}
                  onChange={(value) => setActiveInputEngine(value as VoiceEngineKind | '')}
                  options={[
                    { value: '', label: 'Not selected' },
                    ...surface.engines
                      .filter((engine) => engine.installed && engine.supports_input)
                      .map((engine) => ({
                        value: engine.kind,
                        label: `${engine.name} (${engine.readiness})`,
                      })),
                  ]}
                />
                <SelectField
                  label="Active Output Engine"
                  value={activeOutputEngine}
                  onChange={(value) => setActiveOutputEngine(value as VoiceEngineKind | '')}
                  options={[
                    { value: '', label: 'Not selected' },
                    ...surface.engines
                      .filter((engine) => engine.installed && engine.supports_output)
                      .map((engine) => ({
                        value: engine.kind,
                        label: `${engine.name} (${engine.readiness})`,
                      })),
                  ]}
                />
              </div>

              <div className="grid gap-4 md:grid-cols-2">
                <SelectField
                  label="Input Device"
                  value={selectedInputDeviceId}
                  onChange={setSelectedInputDeviceId}
                  options={[
                    { value: '', label: 'System default input' },
                    ...surface.devices
                      .filter((device) => device.kind === 'input')
                      .map((device) => ({
                        value: device.id,
                        label: `${device.name}${device.default ? ' (default)' : ''}`,
                      })),
                  ]}
                />
                <SelectField
                  label="Output Device"
                  value={selectedOutputDeviceId}
                  onChange={setSelectedOutputDeviceId}
                  options={[
                    { value: '', label: 'System default output' },
                    ...surface.devices
                      .filter((device) => device.kind === 'output')
                      .map((device) => ({
                        value: device.id,
                        label: `${device.name}${device.default ? ' (default)' : ''}`,
                      })),
                  ]}
                />
              </div>
            </section>

            <section className="rounded-xl border border-surface2 bg-surface p-6 space-y-4">
              <h2 className="text-lg font-semibold">Policy</h2>
              <div className="grid gap-3 md:grid-cols-2">
                <CheckboxRow
                  label="Require approval for spoken output"
                  checked={policy.require_approval_for_tts}
                  onChange={(checked) =>
                    setPolicy((current) => ({ ...current, require_approval_for_tts: checked }))
                  }
                />
                <CheckboxRow
                  label="Require approval for speech input"
                  checked={policy.require_approval_for_stt}
                  onChange={(checked) =>
                    setPolicy((current) => ({ ...current, require_approval_for_stt: checked }))
                  }
                />
                <CheckboxRow
                  label="Allow remote audio input"
                  checked={policy.allow_remote_audio_input}
                  onChange={(checked) =>
                    setPolicy((current) => ({ ...current, allow_remote_audio_input: checked }))
                  }
                />
                <CheckboxRow
                  label="Allow remote audio output"
                  checked={policy.allow_remote_audio_output}
                  onChange={(checked) =>
                    setPolicy((current) => ({ ...current, allow_remote_audio_output: checked }))
                  }
                />
                <CheckboxRow
                  label="Persist transcripts"
                  checked={policy.persist_transcripts}
                  onChange={(checked) =>
                    setPolicy((current) => ({ ...current, persist_transcripts: checked }))
                  }
                />
              </div>
            </section>

            <section className="rounded-xl border border-surface2 bg-surface p-6 space-y-4">
              <div className="flex items-start justify-between gap-4">
                <div>
                  <h2 className="text-lg font-semibold">Smoke Tests</h2>
                  <p className="text-sm text-gray-400">
                    Native output can speak through the installed Voice Pack today. Self-hosted
                    engines stay explicit about readiness instead of pretending to run.
                  </p>
                </div>
                <div className="flex gap-3">
                  <button
                    onClick={() => void testInput()}
                    disabled={saving}
                    className="rounded-lg border border-surface2 px-4 py-2 text-sm hover:border-primary disabled:opacity-60"
                  >
                    Test Input
                  </button>
                  <button
                    onClick={() => void testOutput()}
                    disabled={saving}
                    className="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-white hover:bg-primary/80 disabled:opacity-60"
                  >
                    Test Output
                  </button>
                </div>
              </div>
              <label className="block text-sm text-gray-300">
                Spoken output text
                <input
                  value={testText}
                  onChange={(event) => setTestText(event.target.value)}
                  className="mt-2 w-full rounded-lg border border-surface2 bg-background px-3 py-2 text-sm outline-none focus:border-primary"
                />
              </label>
            </section>

            <section className="grid gap-4 lg:grid-cols-3">
              {surface.engines.map((engine) => (
                <EngineCard
                  key={engine.kind}
                  engine={engine}
                  draft={drafts[engine.kind]}
                  saving={saving}
                  onDraftChange={(patch) => updateDraft(engine.kind, patch)}
                  onInstall={() => void installEngine(engine.kind)}
                  onUninstall={() => void uninstallEngine(engine.kind)}
                  onActivateInput={() => void activateInput(engine.kind)}
                  onActivateOutput={() => void activateOutput(engine.kind)}
                />
              ))}
            </section>

            {surface.warnings.length ? (
              <section className="rounded-xl border border-amber-500/40 bg-amber-500/10 p-6 space-y-2">
                <h2 className="text-lg font-semibold text-amber-200">Warnings</h2>
                <ul className="space-y-2 text-sm text-amber-100">
                  {surface.warnings.map((warning) => (
                    <li key={warning}>- {warning}</li>
                  ))}
                </ul>
              </section>
            ) : null}
          </>
        ) : null}
      </main>
    </div>
  );
}

function EngineCard({
  engine,
  draft,
  saving,
  onDraftChange,
  onInstall,
  onUninstall,
  onActivateInput,
  onActivateOutput,
}: {
  engine: VoiceEngineRecord;
  draft: EngineDraft;
  saving: boolean;
  onDraftChange: (patch: Partial<EngineDraft>) => void;
  onInstall: () => void;
  onUninstall: () => void;
  onActivateInput: () => void;
  onActivateOutput: () => void;
}) {
  return (
    <article className="rounded-xl border border-surface2 bg-surface p-5 space-y-4">
      <div className="space-y-1">
        <div className="flex items-center justify-between gap-3">
          <h3 className="text-base font-semibold">{engine.name}</h3>
          <span className="rounded-full border border-surface2 px-2 py-1 text-xs text-gray-300">
            {engine.readiness}
          </span>
        </div>
        <p className="text-xs text-gray-400">
          installed={String(engine.installed)} enabled={String(engine.enabled)} asset=
          {engine.asset_status}
        </p>
      </div>

      <div className="space-y-3">
        {engine.kind === 'local_whisper' ? (
          <Field
            label="Model"
            value={draft.model}
            onChange={(value) => onDraftChange({ model: value })}
            placeholder="tiny"
          />
        ) : null}
        {engine.kind === 'local_piper' || engine.kind === 'native_os' ? (
          <Field
            label="Voice"
            value={draft.voice}
            onChange={(value) => onDraftChange({ voice: value })}
            placeholder={engine.kind === 'local_piper' ? 'en_US-lessac-medium' : 'Optional OS voice'}
          />
        ) : null}
        <Field
          label="Runtime Path"
          value={draft.runtime_path}
          onChange={(value) => onDraftChange({ runtime_path: value })}
          placeholder="Optional binary or runtime override"
        />
        <Field
          label="Notes"
          value={draft.notes}
          onChange={(value) => onDraftChange({ notes: value })}
          placeholder="Optional operator note"
        />
        <CheckboxRow
          label="Engine enabled"
          checked={draft.enabled}
          onChange={(checked) => onDraftChange({ enabled: checked })}
        />
      </div>

      <div className="flex flex-wrap gap-2">
        <button
          onClick={engine.installed ? onUninstall : onInstall}
          disabled={saving}
          className="rounded-lg border border-surface2 px-3 py-2 text-sm hover:border-primary disabled:opacity-60"
        >
          {engine.installed ? 'Uninstall' : 'Install'}
        </button>
        <button
          onClick={onActivateInput}
          disabled={saving || !engine.installed || !engine.supports_input}
          className="rounded-lg border border-surface2 px-3 py-2 text-sm hover:border-primary disabled:opacity-60"
        >
          {engine.active_input ? 'Input Active' : 'Activate Input'}
        </button>
        <button
          onClick={onActivateOutput}
          disabled={saving || !engine.installed || !engine.supports_output}
          className="rounded-lg border border-surface2 px-3 py-2 text-sm hover:border-primary disabled:opacity-60"
        >
          {engine.active_output ? 'Output Active' : 'Activate Output'}
        </button>
      </div>

      {engine.warnings.length ? (
        <ul className="space-y-2 text-xs text-amber-200">
          {engine.warnings.map((warning) => (
            <li key={warning}>- {warning}</li>
          ))}
        </ul>
      ) : null}
    </article>
  );
}

function configuredEngineInputs(
  engines: VoiceEngineRecord[],
  drafts: Record<VoiceEngineKind, EngineDraft>,
): VoiceEngineInput[] {
  return engines
    .filter(
      (engine) =>
        engine.installed ||
        Boolean(drafts[engine.kind].model) ||
        Boolean(drafts[engine.kind].voice) ||
        Boolean(drafts[engine.kind].runtime_path) ||
        Boolean(drafts[engine.kind].notes),
    )
    .map((engine) => ({
      kind: engine.kind,
      enabled: drafts[engine.kind].enabled,
      model: drafts[engine.kind].model || null,
      voice: drafts[engine.kind].voice || null,
      runtime_path: drafts[engine.kind].runtime_path || null,
      asset_dir: engine.asset_dir ?? null,
      notes: drafts[engine.kind].notes || null,
    }));
}

function buildDrafts(engines: VoiceEngineRecord[]): Record<VoiceEngineKind, EngineDraft> {
  const next = emptyDrafts();
  for (const engine of engines) {
    next[engine.kind] = {
      model: engine.model ?? '',
      voice: engine.voice ?? '',
      runtime_path: engine.runtime_path ?? '',
      notes: engine.notes ?? '',
      enabled: engine.enabled,
    };
  }
  return next;
}

function emptyDrafts(): Record<VoiceEngineKind, EngineDraft> {
  return {
    native_os: { ...EMPTY_DRAFT },
    local_whisper: { ...EMPTY_DRAFT },
    local_piper: { ...EMPTY_DRAFT },
  };
}

function Field({
  label,
  value,
  onChange,
  placeholder,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
}) {
  return (
    <label className="block text-sm text-gray-300">
      {label}
      <input
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        className="mt-2 w-full rounded-lg border border-surface2 bg-background px-3 py-2 text-sm outline-none focus:border-primary"
      />
    </label>
  );
}

function SelectField({
  label,
  value,
  onChange,
  options,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  options: Array<{ value: string; label: string }>;
}) {
  return (
    <label className="block text-sm text-gray-300">
      {label}
      <select
        value={value}
        onChange={(event) => onChange(event.target.value)}
        className="mt-2 w-full rounded-lg border border-surface2 bg-background px-3 py-2 text-sm outline-none focus:border-primary"
      >
        {options.map((option) => (
          <option key={`${label}-${option.value}`} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
    </label>
  );
}

function CheckboxRow({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="flex items-center gap-3 rounded-lg border border-surface2 bg-background/50 px-4 py-3 text-sm text-gray-200">
      <input type="checkbox" checked={checked} onChange={(event) => onChange(event.target.checked)} />
      {label}
    </label>
  );
}

function StatCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl border border-surface2 bg-surface px-4 py-5">
      <p className="text-xs uppercase tracking-wide text-gray-400">{label}</p>
      <p className="mt-2 text-lg font-semibold">{value}</p>
    </div>
  );
}

function daemonClient() {
  return new RoveDaemonClient(readStoredToken() ?? undefined);
}

function formatError(error: unknown) {
  if (error instanceof DaemonError) {
    return error.message;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return 'Unknown voice surface error';
}
