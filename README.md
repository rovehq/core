<div align="center">

<a name="readme-top"></a>

# Rove Core

Local-first, self-hosted AI infrastructure. A single signed daemon runs your<br/>
tasks, memory, brains, and remote mesh — on your machine, under your keys.

[![License][repo_license_img]][repo_license_url]
[![Channel: dev][channel_dev_img]][channel_dev_url]
[![Channel: stable][channel_stable_img]][channel_stable_url]

**&searr;&nbsp;&nbsp;Install in one line, pick your channel&nbsp;&nbsp;&swarr;**

</div>

## ✨ Features

- **Local-first by default.** The daemon listens on `127.0.0.1:47630` and never phones home.
- **Two release channels.** `dev` auto-updates at 00:00 UTC from the latest nightly; `stable` is manual-update only.
- **Signed everything.** Engine, core-tools, plugins, drivers, brains — each manifest is Ed25519-signed with BLAKE3 hashes.
- **Dual trust tiers.** Official artifacts are signed by the Rove team key; community WASM plugins are signed by a separate community key with its own blast radius.
- **One daemon, two profiles.** `desktop` for local interactive use, `headless` for servers and agents.
- **Remote mesh.** Stable node identity, signed remote requests, replay protection, optional ZeroTier transport hints.
- **Hosted WebUI shell.** Ship the UI from `app.roveai.co` — daemon stays lean, UI updates independently.
- **Cross-platform.** Linux (x86_64), macOS (aarch64), Windows (x86_64). More targets as they stabilize.

## ⚡️ Quick start

No package managers, no installer wizards. One line.

### 🐧 Linux / macOS

Install the **dev** channel (default for now, since Rove is pre-stable):

```console
curl -fsSL https://rove.sh/install.sh | ROVE_CHANNEL=dev sh
```

Install the **stable** channel (empty until `v*` tags ship):

```console
curl -fsSL https://rove.sh/install.sh | sh
```

The binary lands in `/usr/local/bin/rove-dev` (dev) or `/usr/local/bin/rove` (stable), and the daemon root is `$HOME/.rove-dev` or `$HOME/.rove` respectively — so both channels can coexist on one machine.

<div align="right">

[&nwarr; Back to top](#readme-top)

</div>

### 🪟 Windows

Install via PowerShell:

```powershell
# dev channel
$env:ROVE_CHANNEL="dev"; irm https://rove.sh/install.ps1 | iex

# stable channel
irm https://rove.sh/install.ps1 | iex
```

<div align="right">

[&nwarr; Back to top](#readme-top)

</div>

### 📦 Package installers

Homebrew, MSI, `.deb`, `.rpm`, and Arch packages are on the roadmap but not shipped yet. For now, the install scripts above are the only supported path.

<div align="right">

[&nwarr; Back to top](#readme-top)

</div>

### 🗑️ Uninstall

```console
# Linux / macOS
curl -fsSL https://rove.sh/uninstall.sh | sh

# Windows
irm https://rove.sh/uninstall.ps1 | iex
```

Removes the binary, config, data, cache, plugins, database, and any daemon service.

<div align="right">

[&nwarr; Back to top](#readme-top)

</div>

## 🗂️ Workspace layout

| Path | Purpose |
| ---- | ------- |
| `engine/` | Runtime core: daemon, CLI, TUI, storage, policy, security, services, remote mesh |
| `brain/` | Dispatch and reasoning runtimes, adapters, local-model helpers |
| `sdk/` | Shared contracts: tools, extensions, brains, policy, services, remote control |
| `tools/` | Core Rust tools built as dynamic libraries (`filesystem`, `screenshot`, `terminal`, `ui-server`, …) |
| `webui/` | Hosted-shell WebUI client for `app.roveai.co` + local daemon dev |
| `scripts/` | Install, uninstall, key generation, release helpers |
| `manifest/` | Embedded public keys resolved at build time (official + community) |
| `docs/` | Developer docs and reports |

## 🧭 Channels

Rove ships two channels in parallel. Each has its own binary name, its own daemon root, its own registry prefix, and its own update policy.

| Channel | Binary | Daemon root | Registry prefix | Auto-update |
| --- | --- | --- | --- | --- |
| `stable` | `rove` | `$HOME/.rove` | `registry.roveai.co/stable/...` | Manual only (`rove update`) |
| `dev` | `rove-dev` | `$HOME/.rove-dev` | `registry.roveai.co/dev/...` | Daily at **00:00 UTC** |

CI builds pre-fetch nightly dev artifacts at **22:00 UTC** (2 hours before the auto-update window), so by 00:00 UTC, signed manifests are already on R2 and ready to serve.

Pre-stable note: while no `v*` tag has been cut, every push to `main` ships to the `dev` channel. Tag-based stable releases are gated on the checklist in [`.report/STATUS.md`](../.report/STATUS.md).

<div align="right">

[&nwarr; Back to top](#readme-top)

</div>

## 🏗️ Current architecture

- One daemon binary, two profiles: `desktop` and `headless`.
- Control surface: `task`, `brain`, `policy`, `extension`, `service`, `remote`.
- Local daemon auth with bearer sessions and reauth windows.
- Installable system extensions (no legacy built-in tools).
- Remote mesh with stable node identity, signed requests, replay protection.
- Signed artifacts verified at install + update time via the embedded public keys.
- Hosted WebUI shell — no daemon-served UI bundle.

## 🧪 Development

```bash
# Build the workspace
cargo check --all-targets
cargo test --workspace

# Run the daemon locally (desktop profile, default port)
cargo run -p engine --bin rove -- daemon --profile desktop --port 47630

# Build the WebUI shell
(cd webui && npm ci && npm run build)
```

Useful entry points:

- [`engine/README.md`](./engine/README.md)
- [`webui/README.md`](./webui/README.md)
- [`scripts/README.md`](./scripts/README.md)
- [`../.report/STATUS.md`](../.report/STATUS.md) — current verified feature state
- [`../.docs/agent-playbook.md`](../.docs/agent-playbook.md) — how agents work in this repo

## 🔐 Security model

- Every manifest is Ed25519-signed over the canonicalized JSON (signature field and timestamps excluded from canonical form).
- Official signatures are produced by the Rove team key, held in Infisical; community signatures by a separate community key.
- Community authors do not hold keys — CI signs with the community key only after PR merge.
- Public keys are pinned into the engine at build time via `build.rs` and resolved from either GitHub secrets (CI) or the `manifest/*.bin` files (local).
- BLAKE3 is the content hash for all artifacts; SHA-256 is the emergency fallback.

## 🤝 Contributing

- [Issues][repo_issues_url] — bugs and feature requests
- [Pull requests][repo_pull_request_url] — send improvements against `main`
- [Discussions][repo_discussions_url] — questions and ideas

Agents (Claude, Codex, Cursor, etc.) should read [`AGENTS.md`](../AGENTS.md) and [`.docs/agent-playbook.md`](../.docs/agent-playbook.md) before making changes.

<div align="right">

[&nwarr; Back to top](#readme-top)

</div>

<!-- Links -->
[repo_license_url]: https://github.com/rovehq/core/blob/main/LICENSE
[repo_license_img]: https://img.shields.io/badge/license-Apache_2.0-blue
[channel_dev_img]: https://img.shields.io/badge/channel-dev-yellow
[channel_dev_url]: https://registry.roveai.co/dev/engine/manifest.json
[channel_stable_img]: https://img.shields.io/badge/channel-stable-green
[channel_stable_url]: https://registry.roveai.co/stable/engine/manifest.json
[repo_issues_url]: https://github.com/rovehq/core/issues
[repo_pull_request_url]: https://github.com/rovehq/core/pulls
[repo_discussions_url]: https://github.com/rovehq/core/discussions
