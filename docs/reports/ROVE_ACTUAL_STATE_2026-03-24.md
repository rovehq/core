# Rove Actual State Report

Date: March 24, 2026

Baseline commit: `e0e5dab` (`add agent factory command center and telegram pack`)

## Executive Summary

Rove is no longer just a collection of architectural plans. In the current
repository, it is a real daemon-centered runtime with:

- one authenticated local daemon
- `desktop` and `headless` runtime profiles
- immutable node identity
- approvals and allowlist controls
- vault-backed secret handling
- signed remote execution and peer trust
- ZeroTier as an official remote transport surface
- a signed extension catalog/update model
- first-class `AgentSpec` and `WorkflowSpec`
- an initial agent/workflow factory layer
- a WebUI command center
- a first productized channel surface for Telegram

The strongest current truth is this:

- Rove is already credible as infrastructure
- Rove is not yet complete as a polished "replace all" product
- the remaining work is mostly product depth, reliability hardening, and
  operator polish, not invention of the underlying runtime model

## Source Of Truth Used For This Report

This report is based on the repository state after `e0e5dab`, not on earlier
roadmap prose alone.

Evidence used:

- current commit trail on `main`
- current CLI/API/WebUI codepaths
- focused verification commands run against the current tree
- `code-review-graph status`

## Code Review Graph Snapshot

From `code-review-graph status` on March 24, 2026:

- Nodes: 3423
- Edges: 31209
- Files: 326
- Languages: rust, python, typescript, javascript, tsx
- Last updated: 2026-03-23T23:18:50

This is useful as a scale signal only. The source tree below is the actual
behavioral truth.

## What Rove Actually Has Today

### 1. One real daemon runtime

The daemon model is real, not aspirational.

- One daemon binary exists.
- It supports profile-aware startup.
- Hosted WebUI talks to the local daemon instead of being its own runtime.
- CLI, daemon, and WebUI share the same runtime objects and control plane.

Primary code:

- [`engine/src/main.rs`](../../engine/src/main.rs)
- [`engine/src/api/server/mod.rs`](../../engine/src/api/server/mod.rs)
- [`engine/src/api/server/api.rs`](../../engine/src/api/server/api.rs)

### 2. Profile-aware operation

Rove has a real profile split:

- `desktop`
- `headless`

This is part of config and runtime behavior, not a naming placeholder.

Primary code:

- [`engine/src/config`](../../engine/src/config)
- [`engine/src/cli/commands.rs`](../../engine/src/cli/commands.rs)

### 3. Protected node identity and trust model

Rove has an actual node identity layer:

- stable `node_id`
- renameable `node_name`
- public/private signing identity
- remote trust tied to Rove identity, not transient transport metadata

Primary code:

- [`engine/src/system/identity.rs`](../../engine/src/system/identity.rs)
- [`engine/src/system/remote.rs`](../../engine/src/system/remote.rs)

### 4. Approvals, policy, and secrets are real runtime layers

The daemon has:

- approval modes
- approval rules
- pending approvals
- policy evaluation
- secret backend selection
- vault-backed secret persistence

This is runtime behavior, not just UI text.

Primary code:

- [`engine/src/security/approvals.rs`](../../engine/src/security/approvals.rs)
- [`engine/src/security/secrets`](../../engine/src/security/secrets)
- [`engine/src/policy`](../../engine/src/policy)

### 5. Remote mesh exists

Remote execution is materially present:

- peer pairing
- explicit trust
- signed remote requests
- replay protection
- remote status surfaces
- remote send flow

Primary code:

- [`engine/src/system/remote.rs`](../../engine/src/system/remote.rs)
- [`engine/src/api/server/ws.rs`](../../engine/src/api/server/ws.rs)

### 6. ZeroTier is now a real maintained remote surface

Rove has an official ZeroTier transport/plugin surface under remote/channel
operations.

Current repo reality:

- install/setup/join/status/refresh surfaces exist
- discovery/pair/trust surfaces exist
- transport records and discovery cache exist
- WebUI and API surfaces exist

Primary code:

- [`engine/src/system/zerotier.rs`](../../engine/src/system/zerotier.rs)
- [`engine/src/storage/remote_discovery.rs`](../../engine/src/storage/remote_discovery.rs)
- [`webui/src/app/remote/page.tsx`](../../webui/src/app/remote/page.tsx)

### 7. Extension ecosystem is not raw anymore

Rove has an actual extension catalog/update model:

- public catalog default
- trust badges
- provenance
- update availability
- developer-mode gating for advanced/unverified flows

Primary code:

- [`engine/src/storage/extension_catalog.rs`](../../engine/src/storage/extension_catalog.rs)
- [`engine/src/cli/extensions.rs`](../../engine/src/cli/extensions.rs)
- [`webui/src/app/plugins/page.tsx`](../../webui/src/app/plugins/page.tsx)

### 8. Persisted config is versioned

Persisted config now has explicit schema/version tracking and a typed SDK view.

Primary code:

- [`engine/src/config/core.rs`](../../engine/src/config/core.rs)
- [`engine/src/config/mod.rs`](../../engine/src/config/mod.rs)
- [`sdk/src/config_handle.rs`](../../sdk/src/config_handle.rs)

### 9. Builtin core tools are protected again

Rove now protects builtin tool precedence and fixed the reactor/runtime bugs in
native tool adapters.

That means core file/terminal/screenshot capabilities are no longer allowed to
silently degrade behind stale installed system extensions.

Primary code:

- [`engine/src/runtime/registry.rs`](../../engine/src/runtime/registry.rs)
- [`engine/src/runtime/builtin/mod.rs`](../../engine/src/runtime/builtin/mod.rs)
- [`tools/filesystem/src/lib.rs`](../../tools/filesystem/src/lib.rs)
- [`tools/terminal/src/lib.rs`](../../tools/terminal/src/lib.rs)
- [`tools/screenshot/src/lib.rs`](../../tools/screenshot/src/lib.rs)

### 10. First-class agent and workflow runtime exists

This is one of the biggest actual changes in the repo.

Rove now has:

- `AgentSpec`
- `WorkflowSpec`
- persistent spec storage
- run history
- CLI/API/WebUI management
- default assistant as a spec, not a hidden special case

Primary code:

- [`sdk/src/agent_spec.rs`](../../sdk/src/agent_spec.rs)
- [`engine/src/system/specs.rs`](../../engine/src/system/specs.rs)
- [`engine/src/storage/agent_runs.rs`](../../engine/src/storage/agent_runs.rs)
- [`engine/src/cli/agents.rs`](../../engine/src/cli/agents.rs)
- [`engine/src/cli/workflows.rs`](../../engine/src/cli/workflows.rs)
- [`webui/src/app/agents/page.tsx`](../../webui/src/app/agents/page.tsx)
- [`webui/src/app/workflows/page.tsx`](../../webui/src/app/workflows/page.tsx)

### 11. Agent factory now exists

Rove now has an initial factory layer on top of those specs.

Current actual behavior:

- preview and create generated agents
- preview and create generated workflows
- list templates
- create agent/workflow from a successful task
- generated specs are explicit, versioned, and disabled by default

Important limitation:

- this factory is currently deterministic/template-driven
- it is not yet the full dynamic prompt-to-agent factory vision

Primary code:

- [`engine/src/system/factory.rs`](../../engine/src/system/factory.rs)
- [`engine/src/cli/commands.rs`](../../engine/src/cli/commands.rs)
- [`engine/src/api/server/api.rs`](../../engine/src/api/server/api.rs)

### 12. The WebUI home page is now a command center

The root page is no longer just a task box.

Current actual behavior:

- aggregated overview API
- recent tasks
- recent agent/workflow runs
- approvals
- channels
- services
- remote snapshot
- extension update snapshot
- bounded daemon log tail
- task-to-agent / task-to-workflow conversion buttons

Primary code:

- [`engine/src/api/server/api.rs`](../../engine/src/api/server/api.rs)
- [`webui/src/app/page.tsx`](../../webui/src/app/page.tsx)

### 13. Telegram is now a first-class channel surface

There is now a real runtime `channel` noun and a dedicated Telegram control
surface.

Current actual behavior:

- `rove channel list`
- `rove channel telegram status`
- `rove channel telegram setup`
- `rove channel telegram enable`
- `rove channel telegram disable`
- `rove channel telegram test`
- `rove channel telegram doctor`
- dedicated Channels page in WebUI
- default Telegram handler binds to a real enabled `AgentSpec`

Primary code:

- [`engine/src/cli/channel.rs`](../../engine/src/cli/channel.rs)
- [`engine/src/channels/manager.rs`](../../engine/src/channels/manager.rs)
- [`engine/src/channels/telegram`](../../engine/src/channels/telegram)
- [`webui/src/app/channels/page.tsx`](../../webui/src/app/channels/page.tsx)

## What Is Partially Done

These are real, but not fully finished.

### Telegram productization is present, but not fully deep

Done:

- setup/status/test/doctor/binding surfaces exist
- default handler agent binding exists
- WebUI and CLI both expose it

Not fully done:

- live end-to-end inbound verification still depends on real bot credentials
- approval/admin-chat behavior is only partially realized in the current bound
  agent path
- there is not yet a richer multi-agent Telegram routing model

### Command center is real, but not a full live ops console yet

Done:

- overview endpoint
- panels for major runtime surfaces
- log tail
- recent runs

Not fully done:

- no rich streaming operator console
- no deep fleet dashboard
- no advanced per-surface live drill-down from `/`

### Agent factory is real, but still early

Done:

- template-backed generation
- from-task generation
- structured explicit output

Not fully done:

- no LLM-backed requirement compiler
- no review/approval workflow for generated specs beyond manual editing
- no graph-to-agent or graph-to-workflow path

## What Is Not Done Yet

### 1. Agent Studio

There is no visual canvas/editor yet for:

- instructions blocks
- model blocks
- memory blocks
- channel blocks
- schedule blocks
- MCP blocks
- output blocks

### 2. Full dynamic agent factory

Rove does not yet have the full "describe the agent and Rove compiles it into a
runtime object" experience envisioned in the roadmap.

Current factory is a strong first slice, but it is not the final form.

### 3. More channel packs

Telegram is the first productized channel.

Still not done:

- Slack pack
- Discord pack
- broader multi-channel routing/productization

### 4. Full replace-all surface layer

Rove is infrastructure-first and that foundation is real.

But it still does not fully replace:

- polished assistant shells
- background workforce dashboards
- visual agent builders
- creator/research workflow marketplaces
- mobile monitor/approval shells

### 5. Some platform finish work

Still incomplete:

- Windows service install support
- broader desktop/mobile shell finish beyond current surfaces
- more install/doctor polish for first-run paths

## Practical Reliability Notes

These are important because they separate architectural existence from practical
operator confidence.

Recent hardening that is actually in the repo:

- daemon startup now surfaces real data-dir errors
- builtin core tools were restored as authoritative
- native tool reactor/runtime dependency bugs were fixed
- hosted WebUI daemon port handling was corrected and made user-configurable

This means Rove is much more credible as a real daily runtime than the older
plans alone would imply.

## Verified In This Pass

These checks were run against the current repo state while producing this
report:

- `code-review-graph status`
- `cargo check -p engine`
- `cargo clippy -p engine -- -D warnings`
- `cargo test -p engine preview_agent_is_disabled_and_explicit --lib -- --nocapture`
- `cargo test -p engine from_task_generates_specs --lib -- --nocapture`
- `npm run build` in `core/webui`

One test remains environment-blocked in this sandbox:

- `cargo test -p engine test_telegram_bot_creation --lib -- --nocapture`

Observed failure:

- macOS `system-configuration` null-object issue in this environment

That is an environment/runtime issue, not evidence that the new factory or
command-center code failed to compile.

## Current Best Reading Of Rove

If someone asks "what is Rove today?", the honest answer is:

Rove is already a real daemon-centered agent infrastructure platform with
identity, approvals, secrets, remote execution, installable capabilities,
first-class agent/workflow specs, an initial generation layer, a command center,
and a first productized channel pack.

If someone asks "what is Rove not yet?", the honest answer is:

Rove is not yet the fully polished replace-all agent operating system with
visual composition, broad channel coverage, and dynamic agent creation depth.

## Recommended Next Steps

1. Deepen Telegram with real end-to-end acceptance, better approval routing,
   and cleaner operator diagnostics.
2. Build the next factory layer: richer prompt-to-spec compilation and better
   review UX.
3. Start Agent Studio as a structured editor over `AgentSpec` and
   `WorkflowSpec`, not a separate runtime.
4. Productize the next channels only after Telegram is boringly dependable.
5. Keep prioritizing practical runtime truth over surface expansion.
