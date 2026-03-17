# Rove Core

Rove Core contains the Rust engine, the local brain runtime, and the SDK used
by plugins and supporting services.

## Crates

| Path | Purpose |
| ---- | ------- |
| `engine/` | Runtime core: daemon, CLI, storage, security, tools, and APIs |
| `brain/` | Local reasoning helpers and runtime management for llama.cpp |
| `sdk/` | Shared contracts for tools, plugins, manifests, and task types |

## Development

```bash
cargo check --all-targets
cargo test --all
```

The web surfaces in this repo are intentionally deferred from the Rust-first
rebuild and are only touched when the engine build requires them.
