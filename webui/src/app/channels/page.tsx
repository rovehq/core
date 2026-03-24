'use client';

import { type ReactNode, useEffect, useState } from 'react';

import Nav from '@/components/Nav';
import {
  AgentSpec,
  DaemonError,
  readStoredToken,
  RoveDaemonClient,
  TelegramChannelStatus,
  TelegramChannelTestResponse,
} from '@/lib/daemon';

export default function ChannelsPage() {
  const [agents, setAgents] = useState<AgentSpec[]>([]);
  const [status, setStatus] = useState<TelegramChannelStatus | null>(null);
  const [token, setToken] = useState('');
  const [allowedIds, setAllowedIds] = useState('');
  const [confirmationChatId, setConfirmationChatId] = useState('');
  const [apiBaseUrl, setApiBaseUrl] = useState('');
  const [defaultAgentId, setDefaultAgentId] = useState('');
  const [testResult, setTestResult] = useState<TelegramChannelTestResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void refresh();
  }, []);

  async function refresh() {
    setLoading(true);
    setError(null);
    try {
      const client = daemonClient();
      const [nextStatus, nextAgents] = await Promise.all([
        client.getTelegramChannel(),
        client.listAgents(),
      ]);
      setStatus(nextStatus);
      setAgents(nextAgents.filter((agent) => agent.enabled));
      setAllowedIds(nextStatus.allowed_ids.join(', '));
      setConfirmationChatId(nextStatus.confirmation_chat_id ? String(nextStatus.confirmation_chat_id) : '');
      setApiBaseUrl(nextStatus.api_base_url ?? '');
      setDefaultAgentId(nextStatus.default_agent_id ?? '');
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setLoading(false);
    }
  }

  async function saveSetup() {
    setSaving(true);
    setError(null);
    try {
      const nextStatus = await daemonClient().setupTelegramChannel({
        token: token.trim() || undefined,
        allowed_ids: parseIntCsv(allowedIds),
        confirmation_chat_id: confirmationChatId.trim()
          ? Number(confirmationChatId.trim())
          : undefined,
        api_base_url: apiBaseUrl.trim() || undefined,
        default_agent_id: defaultAgentId || undefined,
      });
      setToken('');
      setStatus(nextStatus);
      await refresh();
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setSaving(false);
    }
  }

  async function setEnabled(enabled: boolean) {
    setSaving(true);
    setError(null);
    try {
      const nextStatus = enabled
        ? await daemonClient().enableTelegramChannel()
        : await daemonClient().disableTelegramChannel();
      setStatus(nextStatus);
      await refresh();
    } catch (nextError) {
      setError(formatError(nextError));
    } finally {
      setSaving(false);
    }
  }

  async function runTest() {
    setSaving(true);
    setError(null);
    try {
      setTestResult(await daemonClient().testTelegramChannel());
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
            <h1 className="text-2xl font-semibold">Channels</h1>
            <p className="text-sm text-gray-400">
              Productize inbound channels on top of the daemon runtime. Telegram is the first fully managed channel pack and binds to a real enabled agent, not a hidden prompt tunnel.
            </p>
          </div>
          <Nav />
        </div>
      </header>

      <main className="flex-1 max-w-6xl w-full mx-auto px-4 py-6 space-y-6">
        <section className="rounded-xl border border-surface2 bg-surface p-6 space-y-5">
          <div className="flex items-center justify-between gap-3">
            <div>
              <h2 className="text-lg font-semibold">Telegram</h2>
              <p className="text-sm text-gray-400">
                Configure the bot token, allow-list, approval chat, API base URL, and the default inbound handler agent.
              </p>
            </div>
            <button
              onClick={() => void refresh()}
              className="rounded-lg border border-surface px-4 py-2 text-sm hover:border-primary"
            >
              Refresh
            </button>
          </div>

          {loading ? (
            <div className="text-sm text-gray-400">Loading channel state...</div>
          ) : (
            <>
              <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
                <Card label="Enabled" value={status?.enabled ? 'yes' : 'no'} />
                <Card label="Configured" value={status?.configured ? 'yes' : 'no'} />
                <Card label="Can Receive" value={status?.can_receive ? 'yes' : 'no'} />
                <Card label="Default Agent" value={status?.default_agent_name ?? 'none'} />
              </div>

              <div className="grid gap-4 md:grid-cols-2">
                <Field label="Bot token">
                  <input
                    type="password"
                    value={token}
                    onChange={(event) => setToken(event.target.value)}
                    className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                    placeholder={status?.token_configured ? 'Stored in daemon secrets' : '123456:ABC...'}
                  />
                </Field>
                <Field label="Default handler agent">
                  <select
                    value={defaultAgentId}
                    onChange={(event) => setDefaultAgentId(event.target.value)}
                    className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                  >
                    <option value="">Select enabled agent</option>
                    {agents.map((agent) => (
                      <option key={agent.id} value={agent.id}>
                        {agent.name} ({agent.id})
                      </option>
                    ))}
                  </select>
                </Field>
                <Field label="Allowed user ids">
                  <input
                    value={allowedIds}
                    onChange={(event) => setAllowedIds(event.target.value)}
                    className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                    placeholder="123456789, 987654321"
                  />
                </Field>
                <Field label="Approval chat id">
                  <input
                    value={confirmationChatId}
                    onChange={(event) => setConfirmationChatId(event.target.value)}
                    className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                    placeholder="Optional admin / approval chat"
                  />
                </Field>
              </div>

              <Field label="API base url">
                <input
                  value={apiBaseUrl}
                  onChange={(event) => setApiBaseUrl(event.target.value)}
                  className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary"
                  placeholder="https://api.telegram.org"
                />
              </Field>

              <div className="flex flex-wrap gap-3">
                <button
                  onClick={() => void saveSetup()}
                  disabled={saving}
                  className="rounded-lg bg-primary px-4 py-2 font-medium hover:bg-primary/80 disabled:bg-surface2 disabled:text-gray-500"
                >
                  {saving ? 'Saving...' : 'Save Setup'}
                </button>
                <button
                  onClick={() => void setEnabled(true)}
                  disabled={saving}
                  className="rounded-lg border border-surface px-4 py-2 text-sm hover:border-primary"
                >
                  Enable
                </button>
                <button
                  onClick={() => void setEnabled(false)}
                  disabled={saving}
                  className="rounded-lg border border-surface px-4 py-2 text-sm hover:border-primary"
                >
                  Disable
                </button>
                <button
                  onClick={() => void runTest()}
                  disabled={saving}
                  className="rounded-lg border border-surface px-4 py-2 text-sm hover:border-primary"
                >
                  Test
                </button>
              </div>

              {status && status.doctor.length > 0 ? (
                <section className="rounded-xl border border-surface bg-background/30 p-4">
                  <h3 className="font-medium">Doctor</h3>
                  <ul className="mt-3 space-y-2 text-sm text-gray-400">
                    {status.doctor.map((line) => (
                      <li key={line}>- {line}</li>
                    ))}
                  </ul>
                </section>
              ) : null}

              {testResult ? (
                <section className="rounded-xl border border-surface bg-background/30 p-4">
                  <h3 className="font-medium">Latest Test</h3>
                  <p className={`mt-3 text-sm ${testResult.ok ? 'text-success' : 'text-error'}`}>
                    {testResult.message}
                  </p>
                </section>
              ) : null}

              {error ? <ErrorBanner error={error} /> : null}
            </>
          )}
        </section>
      </main>
    </div>
  );
}

function daemonClient() {
  return new RoveDaemonClient(readStoredToken() ?? undefined);
}

function parseIntCsv(value: string): number[] {
  return value
    .split(',')
    .map((item) => item.trim())
    .filter(Boolean)
    .map((item) => Number(item))
    .filter((item) => Number.isInteger(item));
}

function formatError(error: unknown) {
  if (error instanceof DaemonError) {
    return error.message;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

function Card({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl border border-surface bg-background/40 p-4">
      <p className="text-xs uppercase tracking-[0.2em] text-gray-500">{label}</p>
      <p className="mt-3 text-lg font-medium">{value}</p>
    </div>
  );
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="block space-y-2">
      <span className="text-sm font-medium text-gray-300">{label}</span>
      {children}
    </label>
  );
}

function ErrorBanner({ error }: { error: string }) {
  return (
    <div className="rounded-lg border border-error/30 bg-error/10 px-4 py-3 text-sm text-error">
      {error}
    </div>
  );
}
