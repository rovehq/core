# Engine

The engine crate is the runtime core of Rove. It owns the daemon lifecycle,
agent loop, API transports, storage, security gates, and tool execution.

## Module Layout

| Module       | Responsibility |
| ------------ | -------------- |
| `agent/`     | ReAct loop, working memory, preferences, and event flow |
| `api/`       | Gateway, MCP bridge, HTTP server, and WebSocket client |
| `channels/`  | External channels such as Telegram |
| `cli/`       | Command parsing, interactive setup, REPL, and bootstrap |
| `config/`    | Config loading, defaults, validation, and metadata |
| `llm/`       | Provider types, router policy, and provider adapters |
| `memory/`    | Conductor, episodic memory, context assembly, and graph |
| `runtime/`   | Native and WASM plugin runtimes |
| `security/`  | Filesystem, command, injection, crypto, and secret gates |
| `steering/`  | Built-in and workspace steering resolution |
| `storage/`   | SQLite pool, task/event repositories, and memory queries |
| `system/`    | Daemon internals, message bus, and telemetry |
| `tools/`     | Core tool registry and tool implementations |

## Commands

```bash
cargo run -p engine --bin rove
cargo test -p engine
cargo check -p engine --all-targets
```

See [../README.md](../README.md) for the workspace layout.
