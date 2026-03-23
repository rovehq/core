# Rove Core Developer Guide

This directory tracks the current developer-facing documentation for the Rove
Core rebuild.

## Current Snapshot

As of March 23, 2026:

- one daemon binary supports `desktop` and `headless` profiles
- policy has replaced steering as the active internal model
- the hosted WebUI shell talks to a password-protected local daemon
- system tools are installable extensions rather than legacy built-ins
- remote execution supports signed requests, replay protection, service install
  state, and ZeroTier transport hints

## Where To Read First

- [`../../README.md`](../../README.md)
- [`../../engine/README.md`](../../engine/README.md)
- [`../../webui/README.md`](../../webui/README.md)
- [`../../engine/how_it_work.mmd`](../../engine/how_it_work.mmd)
- [`../reports/implementation-status-2026-03-23.md`](../reports/implementation-status-2026-03-23.md)
- [`../reports/repo-cleanup-2026-03-23.md`](../reports/repo-cleanup-2026-03-23.md)

## Build Commands

```bash
cargo build --release
cargo test -p engine
cd /Users/as/workspace/rove/core/webui && npm run build
```

## Code Review Graph Snapshot

From `code-review-graph status` on March 23, 2026:

- nodes: 3423
- edges: 31209
- files: 326
- languages: rust, python, typescript, javascript, tsx

Use that graph as a scale/reference signal, not as the source of truth for the
architecture. The READMEs and status reports below are the authoritative
summary.
