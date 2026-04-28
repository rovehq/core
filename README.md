<a name="readme-top"></a>

# Rove Core

<div align="center">

<img src="./assets/logo/Demo_logo.png" alt="Rove" width="160" />

</div>

Rove is a daemon-first, self-hosted AI infrastructure platform. One signed daemon runs your agents, workflows, memory, brains, and remote mesh — on your machine, under your keys. Not a chat shell. Not a cloud broker. **The Nginx of AI agents.**

[![License: BUSL-1.1][license_img]][license_url]
[![Channel: dev][channel_dev_img]][channel_dev_url]
[![Channel: stable][channel_stable_img]][channel_stable_url]

## ⚡️ Quick start

One line. No package managers. Pick a channel.

### 🐧 Linux / macOS

```console
# Dev channel (default for now — Rove is pre-stable)
curl -fsSL https://get.roveai.co/install.sh | ROVE_CHANNEL=dev sh

# Stable channel (empty until v* tags ship)
curl -fsSL https://get.roveai.co/install.sh | sh
```

### 🪟 Windows (PowerShell)

```powershell
# Dev
$env:ROVE_CHANNEL="dev"; irm https://get.roveai.co/install.ps1 | iex

# Stable
irm https://get.roveai.co/install.ps1 | iex
```

### 🧭 What you get

| Channel | Binary | Daemon root | Update policy |
| --- | --- | --- | --- |
| `stable` | `rove` | `$HOME/.rove` | Manual (`rove update`) |
| `dev`    | `rove-dev` | `$HOME/.rove-dev` | Auto at **00:00 UTC** daily |

Both channels can coexist on one machine. Dev CI pre-builds at **22:00 UTC** so signed manifests are on R2 by the 00:00 UTC auto-update window.

> Homebrew, MSI, `.deb`, `.rpm`, Arch and winget are on the roadmap. Install scripts above are the only supported path today.

### 🗑️ Uninstall

```console
# Linux / macOS
curl -fsSL https://get.roveai.co/uninstall.sh | sh

# Windows
irm https://get.roveai.co/uninstall.ps1 | iex
```

<div align="right">

[&nwarr; Back to top](#readme-top)

</div>

## ✨ Features

Every item below is live in `core/` today — each with a concrete file reference.

### 🏛️ Daemon + control plane
- **Single daemon, two profiles** (`desktop`, `headless`). Shared runtime, one PID, double-start guard, self-healing PID file.
  `core/engine/src/system/daemon/`
- **273 API routes** on a unified control plane — CLI, TUI, Telegram, WebUI, and remote mesh all read the same tables.
  `core/engine/src/api/server/mod.rs`
- **Interactive TUI + REPL**, hosted WebUI shell served from `app.roveai.co`, and a `rove` CLI with 40+ subcommands (`task`, `brain`, `policy`, `extension`, `service`, `remote`, `hook`, `memory`, `knowledge`, `workflow`, `mcp`, …).
  `core/engine/src/cli/`

### 🤖 Agents + workflows
- **APEX multi-agent wave executor** — 1021-line executor orchestrating Researcher → Executor → Verifier waves with preset-driven bounded workers.
  `core/engine/src/agent/core/apex.rs`
- **AgentSpec + WorkflowSpec** as canonical specs. Durable workflow orchestration with variables, conditional branches, cancel semantics, and cron/webhook/file-watch triggers.
  `core/engine/src/system/workflow_runtime.rs`, `workflow_triggers.rs`
- **Lifecycle hooks** — 7 event types with a circuit breaker for runaway handlers.
  `core/engine/src/hooks/mod.rs`
- **Agent factory** generates and previews spec bundles before they execute.

### 🧠 Memory + knowledge
- **Working memory + knowledge graph conductor** with FTS5 full-text search and provenance.
  `core/engine/src/memory/conductor/`, `system/knowledge.rs`
- **CLI + API + WebUI** for ingest, query, export, and management — one source of truth across surfaces.
  `core/engine/src/cli/knowledge.rs`

### 🧩 Plugins + extensions
- **WASM sandbox** (extism) with fuel, memory, and timeout limits.
  `core/engine/src/runtime/wasm/{call,host,inspect,load,restart}.rs`
- **Native tools as dynamic libraries** — `filesystem`, `file-watcher`, `app-launcher`, `screenshot`, `terminal`, `ui-server`, `notification`, `voice-native`, `telegram`.
  `core/tools/`
- **Credential zero-exposure injection** — secrets resolved into WASM calls, scrubbed from outputs before they touch logs.
  `core/engine/src/runtime/wasm/call.rs`
- **Builtin tools are authoritative** — native plugins require `Official` or `Reviewed` trust before they can shadow a builtin.
  `core/engine/src/cli/plugins/validate.rs`

### 🔐 Security + trust
- **Dual-key Ed25519 signing.** Official key signs engine, core-tools, drivers, brains, official plugins. Community key signs community WASM plugins only. Cross-signing rejected.
  `core/engine/src/security/crypto/mod.rs` — 27/27 tests pass
- **BLAKE3 content hashing** for every artifact; SHA-256 emergency fallback.
- **Keychain-backed secrets** on macOS, Linux (libsecret), and Windows.
  `core/engine/src/security/secrets/`
- **Approvals layer** with tier-1 / tier-2 risk gates.
  `core/engine/src/security/approvals.rs`
- **Queryable audit log** via `rove logs security` + `GET /v1/audit`.
- **Signed remote execution** — HMAC headers, nonce, TTL, replay protection.

### 📡 Remote + channels
- **Remote mesh** with stable node identity, signed requests, optional ZeroTier transport hints.
- **Telegram as primary channel** — inbound verification, `telegram_audit_log`, admin IDs, 5-minute approval timeout.
  `core/engine/src/channels/telegram/`
- **Rove as MCP server** — JSON-RPC 2.0 stdio, `rove.execute_agent` meta-tool, protocol version `2024-11-05`.
  `core/engine/src/cli/mcp/serve.rs`
- **Browser automation via CDP** — 2035-line plugin with full Chrome DevTools Protocol, smart tools, session management.
  `core/plugins/browser-cdp/`

### 🧰 Ops + observability
- **Prometheus metrics** (`/metrics`) + **OpenTelemetry traces** (`TaskTraceContext`).
  `core/engine/src/system/metrics.rs`
- **Service install/uninstall/upgrade parity** — launchd, systemd, `sc.exe`, `schtasks.exe`.
  `core/engine/src/system/service_install.rs`
- **Migration wedge** — `--dry-run` and `migrate status` with adapters for OpenClaw, ZeroClaw, Moltis.
  `core/engine/src/cli/migrate.rs`

### 🧱 LLM providers
- Anthropic, OpenAI, Gemini, NVIDIA NIM, Ollama, and fully custom endpoints. Provider router + parser at `core/engine/src/llm/`.

<div align="right">

[&nwarr; Back to top](#readme-top)

</div>

## 🗂️ Workspace layout

| Path | Purpose |
| ---- | ------- |
| `engine/` | Runtime core: daemon, CLI, TUI, storage, policy, security, services, remote mesh |
| `brain/` | Dispatch and reasoning runtimes, adapters, local-model helpers |
| `sdk/` | Shared contracts: tools, extensions, brains, policy, services, remote control |
| `tools/` | Core Rust tools built as dynamic libraries |
| `webui/` | Hosted-shell WebUI client for `app.roveai.co` + local daemon dev |
| `scripts/` | Install, uninstall, key generation, release helpers |
| `manifest/` | Embedded public keys resolved at build time (official + community) |
| `assets/` | Logos, icons, static media |
| `docs/` | Developer docs and reports |

## 🧪 Development

```bash
# Build + test
cargo check --all-targets
cargo test --workspace

# Run daemon (desktop profile, default port)
cargo run -p engine --bin rove -- daemon --profile desktop --port 47630

# WebUI shell
(cd webui && npm ci && npm run build)
```

Useful entry points:

- [`engine/README.md`](./engine/README.md)
- [`webui/README.md`](./webui/README.md)
- [`scripts/README.md`](./scripts/README.md)
- [`../.report/STATUS.md`](../.report/STATUS.md) — current verified feature state
- [`../.docs/agent-playbook.md`](../.docs/agent-playbook.md) — how agents work in this repo

## 📄 License

Rove Core is released under the **[Business Source License 1.1](./LICENSE)** (BUSL-1.1).

- ✅ **Free for non-commercial use** — personal, academic, research, evaluation, hobby projects, and non-profit internal use.
- 💼 **Commercial use requires a license.** If you want to sell, host, or operate Rove as part of a paid product or service, contact **team.rovehq@gmail.com**.
- 🔓 **Auto-converts to Apache 2.0** on the Change Date (**2030-04-26**) — four years after first release.

This is not an OSI-approved open-source license, but the source is fully open and modifiable. Full terms in [`LICENSE`](./LICENSE).

## 🤝 Contributing

- [Issues][repo_issues_url] — bugs and feature requests
- [Pull requests][repo_pull_request_url] — improvements against `main`
- [Discussions][repo_discussions_url] — questions and ideas

Agents (Claude, Codex, Cursor, etc.) should read [`AGENTS.md`](../AGENTS.md) and [`.docs/agent-playbook.md`](../.docs/agent-playbook.md) before making changes.

<div align="right">

[&nwarr; Back to top](#readme-top)

</div>

<!-- Links -->
[license_img]: https://img.shields.io/badge/license-BUSL--1.1-orange
[license_url]: ./LICENSE
[channel_dev_img]: https://img.shields.io/badge/channel-dev-yellow
[channel_dev_url]: https://registry.roveai.co/dev/engine/manifest.json
[channel_stable_img]: https://img.shields.io/badge/channel-stable-green
[channel_stable_url]: https://registry.roveai.co/stable/engine/manifest.json
[repo_issues_url]: https://github.com/rovehq/core/issues
[repo_pull_request_url]: https://github.com/rovehq/core/pulls
[repo_discussions_url]: https://github.com/rovehq/core/discussions
