# SDK

The SDK crate contains the shared contracts used by the engine, installable
extensions, tools, and supporting surfaces.

## Main Surfaces

| Module | Responsibility |
| ------ | -------------- |
| `control.rs` | Public control-plane types for daemon auth, remote nodes, profiles, services, and brains |
| `core_tool.rs` | Trait and context for native core tools |
| `manifest.rs` | Extension and plugin manifest parsing/validation |
| `plugin.rs` | Plugin and runtime-facing plugin types |
| `brain.rs` | Brain-family traits and result types |
| `task.rs` | Task source and task-facing contracts |
| `tool_io.rs` | Tool input/output and error surfaces |
| `types.rs` | Compatibility re-exports for shared tool/task types |
| `*_handle.rs` | Engine capability handles for config, crypto, DB, network, bus, and agents |

## Usage

```toml
[dependencies]
rove-sdk = { path = "../sdk" }
```

```rust
use rove_sdk::core_tool::CoreTool;

pub struct MyTool;

impl CoreTool for MyTool {
    fn name(&self) -> &str { "my-tool" }
}
```

The SDK intentionally carries both the older plugin compatibility types and the
newer control-plane types while the repo finishes its public-surface cleanup.

See [`../README.md`](../README.md) for workspace context.
