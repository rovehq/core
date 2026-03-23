# Engine

The engine crate is the runtime core of Rove. It owns the daemon lifecycle,
agent loop, API control plane, runtime registry, storage, policy resolution,
approval gates, service installation, and remote mesh behavior.

## Module Layout

| Module | Responsibility |
| ------ | -------------- |
| `agent/` | Task loop, DAG execution, subagents, and remote delegation |
| `api/` | Local daemon HTTP and WebSocket server, auth/session routes, and remote control endpoints |
| `channels/` | External channels such as Telegram |
| `cli/` | Command parsing, REPL, daemon commands, service install, approvals, and setup |
| `config/` | TOML config loading, profile presets, defaults, aliases, and validation |
| `llm/` | Provider types, routing, and provider adapters |
| `memory/` | Context assembly, episodic memory, and graph-backed recall |
| `policy/` | Policy loader, built-ins, merge rules, and resolution order |
| `runtime/` | Runtime registry, installable system extensions, native runtime, WASM runtime, and MCP runtime |
| `security/` | Crypto, approvals, secret backends, filesystem/command guards, and risk checks |
| `storage/` | SQLite pool, task/event repositories, sessions, approvals, and remote state |
| `system/` | Node identity, remote mesh, ZeroTier transport hints, service install, and daemon internals |
| `tools/` | Tool registry and built-in tool plumbing |

`steering.rs` remains only as a compatibility shim. New code should use
`policy/`.

## Runtime Concepts

- `desktop` profile: WebUI-first local daemon with `auto` secrets and default
  approval mode.
- `headless` profile: boot-safe daemon defaults with vault-backed secrets,
  allowlist approvals, and executor-oriented remote behavior.
- Node identity is stored separately from editable config. `node_id` is derived
  from the public key; `node_name` remains renameable.
- Approval handling is explicit: `default`, `allowlist`, `open`, and future
  `assisted`.
- Remote transport security is layered above connectivity. Peer trust binds to
  `node_id + public_key`, and signed remote requests carry replay-protection
  data.

## Useful Commands

```bash
cargo run -p engine --bin rove -- daemon --profile desktop
cargo test -p engine
cargo check -p engine --all-targets
```

```bash
rove daemon --profile headless
rove service install boot --profile headless
rove approvals mode set allowlist
rove remote node show
rove config reload
```

See [`../README.md`](../README.md) for workspace layout and
[`../docs/reports/implementation-status-2026-03-23.md`](../docs/reports/implementation-status-2026-03-23.md)
for the current milestone report.
