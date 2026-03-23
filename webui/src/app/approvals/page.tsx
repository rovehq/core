'use client';

import { useEffect, useState } from 'react';

import Nav from '@/components/Nav';
import { useRoveStore } from '@/stores/roveStore';

export default function ApprovalsPage() {
  const {
    approvalRules,
    approvals,
    clearError,
    config,
    error,
    initialize,
    refreshApprovalRules,
    refreshApprovals,
    resolveApproval,
    addApprovalRule,
    removeApprovalRule,
  } = useRoveStore();
  const [id, setId] = useState('');
  const [action, setAction] = useState<'allow' | 'require_approval'>('allow');
  const [tool, setTool] = useState('');
  const [commands, setCommands] = useState('');
  const [paths, setPaths] = useState('');
  const [nodes, setNodes] = useState('');
  const [channels, setChannels] = useState('');

  useEffect(() => {
    void initialize();
    void refreshApprovals();
    void refreshApprovalRules();
  }, [initialize, refreshApprovals, refreshApprovalRules]);

  return (
    <div className="min-h-screen flex flex-col">
      <header className="sticky top-0 z-10 bg-background/80 backdrop-blur border-b border-surface2">
        <div className="max-w-5xl mx-auto px-4 py-4 space-y-4">
          <div>
            <h1 className="text-2xl font-semibold">Approvals</h1>
            <p className="text-sm text-gray-400">Resolve daemon-held risk prompts for WebUI and background task sessions.</p>
          </div>
          <Nav />
        </div>
      </header>

      <main className="flex-1 max-w-5xl w-full mx-auto px-4 py-6 space-y-6">
        <section className="bg-surface rounded-xl p-6 border border-surface2 space-y-4">
          <div className="flex items-center justify-between">
            <div>
              <h2 className="text-lg font-semibold">Approval Rules</h2>
              <p className="text-sm text-gray-400">
                Mode: <span className="font-mono">{config?.approval_mode ?? 'unknown'}</span>
              </p>
            </div>
            <button
              onClick={() => void refreshApprovalRules()}
              className="rounded-lg border border-surface2 px-3 py-2 text-sm hover:border-primary"
            >
              Refresh Rules
            </button>
          </div>
          <form
            className="grid gap-3 md:grid-cols-2"
            onSubmit={async (event) => {
              event.preventDefault();
              const ok = await addApprovalRule({
                id,
                action,
                tool: tool || undefined,
                commands: splitCsv(commands),
                paths: splitCsv(paths),
                nodes: splitCsv(nodes),
                channels: splitCsv(channels),
              });
              if (ok) {
                setId('');
                setTool('');
                setCommands('');
                setPaths('');
                setNodes('');
                setChannels('');
              }
            }}
          >
            <Field label="Rule id">
              <input value={id} onChange={(event) => setId(event.target.value)} className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary" />
            </Field>
            <Field label="Action">
              <select value={action} onChange={(event) => setAction(event.target.value as 'allow' | 'require_approval')} className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary">
                <option value="allow">allow</option>
                <option value="require_approval">require_approval</option>
              </select>
            </Field>
            <Field label="Tool pattern">
              <input value={tool} onChange={(event) => setTool(event.target.value)} className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary" placeholder="run_command" />
            </Field>
            <Field label="Commands (comma separated)">
              <input value={commands} onChange={(event) => setCommands(event.target.value)} className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary" placeholder="git status, git diff" />
            </Field>
            <Field label="Paths (comma separated)">
              <input value={paths} onChange={(event) => setPaths(event.target.value)} className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary" placeholder="/workspace/**" />
            </Field>
            <Field label="Nodes / channels">
              <div className="grid gap-3 sm:grid-cols-2">
                <input value={nodes} onChange={(event) => setNodes(event.target.value)} className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary" placeholder="office-mac" />
                <input value={channels} onChange={(event) => setChannels(event.target.value)} className="w-full rounded-lg border border-surface bg-background px-3 py-2 outline-none focus:border-primary" placeholder="telegram" />
              </div>
            </Field>
            <div className="md:col-span-2">
              <button className="rounded-lg bg-primary px-4 py-2 text-sm font-medium hover:bg-primary/80">
                Save Rule
              </button>
            </div>
          </form>
          {approvalRules.length === 0 ? (
            <p className="text-sm text-gray-400">No approval rules saved.</p>
          ) : (
            <div className="space-y-3">
              {approvalRules.map((rule) => (
                <div key={rule.id} className="rounded-lg bg-surface2 px-4 py-3">
                  <div className="flex items-start justify-between gap-4">
                    <div>
                      <p className="font-medium">{rule.id}</p>
                      <p className="text-sm text-gray-500">
                        {rule.action} · tool {rule.tool ?? '*'} · commands {rule.commands.join(', ') || 'none'} · paths {rule.paths.join(', ') || 'none'}
                      </p>
                    </div>
                    <button
                      onClick={() => void removeApprovalRule(rule.id)}
                      className="rounded-lg border border-error/30 px-3 py-2 text-sm text-error hover:bg-error/10"
                    >
                      Remove
                    </button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </section>
        <section className="bg-surface rounded-xl p-6 border border-surface2 space-y-4">
          <div className="flex items-center justify-between">
            <h2 className="text-lg font-semibold">Pending Approvals</h2>
            <button
              onClick={() => void refreshApprovals()}
              className="rounded-lg border border-surface2 px-3 py-2 text-sm hover:border-primary"
            >
              Refresh
            </button>
          </div>
          {approvals.length === 0 ? (
            <p className="text-sm text-gray-400">No pending approvals.</p>
          ) : (
            <div className="space-y-3">
              {approvals.map((approval) => (
                <div key={approval.id} className="rounded-lg bg-surface2 px-4 py-3">
                  <div className="flex items-start justify-between gap-4">
                    <div>
                      <p className="font-medium">Tier {approval.risk_tier} · {approval.tool_name}</p>
                      <p className="text-sm text-gray-500">Task {approval.task_id}</p>
                      <p className="mt-2 text-sm text-gray-300">{approval.summary}</p>
                      {approval.auto_resolve_after_secs ? (
                        <p className="mt-1 text-xs text-gray-500">
                          Auto resolves in {approval.auto_resolve_after_secs}s if left untouched.
                        </p>
                      ) : null}
                    </div>
                    <div className="flex items-center gap-2">
                      <button
                        onClick={() => void resolveApproval(approval.id, true)}
                        className="rounded-lg bg-primary px-3 py-2 text-sm hover:bg-primary/80"
                      >
                        Approve
                      </button>
                      <button
                        onClick={() => void resolveApproval(approval.id, false)}
                        className="rounded-lg border border-error/30 px-3 py-2 text-sm text-error hover:bg-error/10"
                      >
                        Deny
                      </button>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
          <ErrorBanner error={error} onDismiss={clearError} />
        </section>
      </main>
    </div>
  );
}

function splitCsv(value: string) {
  return value
    .split(',')
    .map((item) => item.trim())
    .filter(Boolean);
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
