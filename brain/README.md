# Brain

The `brain` crate contains the local dispatch and reasoning runtime used by the
engine.

## Modules

| Module | Responsibility |
| ------ | -------------- |
| `dispatch/` | Fast task classification and route hints |
| `reasoning/` | Reasoning-family model management and execution helpers |
| `runtime.rs` | Shared model loading and inference plumbing |
| `adapter.rs` | Provider-agnostic interfaces for local/runtime backends |
| `budget.rs` | Token and model-budget helpers |

## Role In The System

- The dispatch family is used for fast task classification and routing.
- Reasoning families back slower, higher-capability execution paths.
- The engine chooses which family to invoke; this crate provides the runtime
  plumbing and helpers rather than the task loop itself.

See [`../README.md`](../README.md) for the workspace overview.
