# 🧠 Brain

AI model adapters for Rove's dispatch and reasoning pipeline.

## 📦 Modules

| Module        | Description                             |
| ------------- | --------------------------------------- |
| `dispatch.rs` | 📡 Intent classification & task routing |
| `runtime.rs`  | ⚙️ Model loading & inference execution  |
| `adapter.rs`  | 🔌 Provider-agnostic model interface    |
| `budget.rs`   | 💰 Token budget management              |

## 🧬 How It Works

```
User Input → Dispatch Brain → Classify Intent
                                    │
                              ┌─────▼──────┐
                              │  Reasoning  │
                              │    Brain    │
                              └─────┬──────┘
                                    │
                              Task Execution
```

The **dispatch brain** (lightweight, fast) classifies intent, then hands off to the **reasoning brain** (powerful, slower) for complex tasks.

---

⬆️ [Back to root](../README.md)
