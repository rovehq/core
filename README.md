# Rove Core

Rove Core is the local execution layer for Rove. It contains the Rust engine,
the local brain runtime, the shared SDK, and the hosted-shell WebUI client that
talks to the daemon on `127.0.0.1:47630`.

## Workspace Layout

| Path | Purpose |
| ---- | ------- |
| `engine/` | Runtime core: daemon, CLI, storage, policy, security, services, and remote mesh |
| `brain/` | Dispatch and reasoning runtimes, adapters, and local-model helpers |
| `sdk/` | Shared contracts for tools, extensions, brains, policy, services, and remote control |
| `webui/` | Hosted-shell WebUI client for `app.roveai.co` and local daemon development |
| `docs/` | Developer docs and progress reports for the current rebuild |

## Current Architecture

- One daemon binary with two runtime profiles: `desktop` and `headless`.
- Public control surface centered on `task`, `brain`, `policy`, `extension`,
  `service`, and `remote`.
- Local daemon auth/session model with bearer sessions and reauth windows.
- Installable system extensions instead of legacy built-in system tools.
- Remote mesh with stable node identity, signed remote requests, replay
  protection, and optional ZeroTier transport hints.
- Hosted WebUI shell backed by the local daemon rather than a daemon-served UI
  bundle.

## Development

```bash
cargo check --all-targets
cargo test --all
cd webui && npm run build
```

Start the daemon locally:

```bash
cargo run -p engine --bin rove -- daemon --profile desktop --port 47630
```

Useful docs:

- [`engine/README.md`](./engine/README.md)
- [`webui/README.md`](./webui/README.md)
- [`docs/dev/index.md`](./docs/dev/index.md)
- [`docs/reports/implementation-status-2026-03-23.md`](./docs/reports/implementation-status-2026-03-23.md)
