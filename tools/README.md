# 🔧 Core Tools

Official native tools that ship with Rove. Built as dynamic libraries, loaded at runtime via FFI.

## 📦 Available Tools

| Tool                                 | Description               |
| ------------------------------------ | ------------------------- |
| 🤖 [`telegram/`](./telegram)         | Telegram bot integration  |
| 🖥️ [`ui-server/`](./ui-server)       | Local web UI server       |
| 📸 [`screenshot/`](./screenshot)     | Screen capture utility    |
| 📂 [`file-watcher/`](./file-watcher) | Filesystem change watcher |
| 🔔 [`notification/`](./notification) | System notifications      |
| 🚀 [`app-launcher/`](./app-launcher) | Application launcher      |
| 🌐 [`remote/`](./remote)             | Remote API connector      |

## 🏗️ Building

```bash
# Build all tools
cargo build --release -p telegram -p ui-server

# Build specific tool
cargo build --release -p telegram
```

## 🧩 Creating a New Tool

1. Create a new crate in `tools/`
2. Depend on `rove-sdk`
3. Implement the `CoreTool` trait
4. Add to workspace `Cargo.toml`

---

⬆️ [Back to root](../README.md)
