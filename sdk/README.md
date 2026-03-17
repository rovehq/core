# 🧰 SDK

Shared types, traits, and helpers for building Rove tools and plugins.

## 📦 What's Inside

| Module         | Description                                  |
| -------------- | -------------------------------------------- |
| `core_tool.rs` | 🔧 Trait for native core tools               |
| `types.rs`     | 📋 Shared types (ToolCall, TaskResult, etc.) |
| `manifest.rs`  | 📝 Plugin manifest parsing & validation      |
| `errors.rs`    | ⚠️ Common error types                        |
| `helpers.rs`   | 🛠️ Utility functions                         |

## 🔗 Usage

```toml
# In your tool's Cargo.toml
[dependencies]
rove-sdk = { path = "../sdk" }
```

```rust
use rove_sdk::core_tool::CoreTool;

pub struct MyTool;

impl CoreTool for MyTool {
    fn name(&self) -> &str { "my-tool" }
    // ...
}
```

## 📖 Examples

- [`core_tool_example.rs`](./examples/core_tool_example.rs) — Building a core tool
- [`tool_usage.rs`](./examples/tool_usage.rs) — Using SDK types

---

⬆️ [Back to root](../README.md)
