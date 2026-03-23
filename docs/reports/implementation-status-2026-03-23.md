# Rove Core Implementation Status

Date: March 23, 2026

## Executive Summary

The kernel-first rebuild is substantially real in this repository. The engine,
remote mesh, local auth, hosted WebUI shell, daemon profiles, approval modes,
identity store, service install controls, and vault-backed secret plumbing are
present. The main remaining gaps are cross-platform finish work, some product
polish, and a few transport/runtime depth items rather than missing core
architecture.

## Review Graph Snapshot

`code-review-graph status` on March 23, 2026:

- Nodes: 3423
- Edges: 31209
- Files: 326
- Languages: rust, python, typescript, javascript, tsx

This is useful as a scale snapshot for the codebase. It does not replace the
source-tree documentation below.

## Major Milestones Already Landed

Recent commit trail on `main`:

| Commit | Summary |
| ------ | ------- |
| `77c1550` | Service install login/boot flow, signed remote transport controls, ZeroTier status/join surfaces, WebUI controls |
| `350b2ae` | Daemon profiles, immutable node identity, approval modes/rules, vault-backed secret backend controls |
| `141191a` | Remote approvals, operator-facing WebUI pages, load-aware scheduling surfaces |
| `1cc5950` | Daemon startup fix for existing databases and release build symbol collision fix |
| `cb206ef` | Local daemon TLS support and WebUI control panels |
| `9cbffda` | Local auth/session model and hosted-shell WebUI client |
| `5df30bd` | Policy rename cleanup from steering to policy |
| `58a4663` | Policy internal cleanup and compatibility alias reduction |
| `b731f30` | Bundled remote execution plans |
| `71ffb15` | Remote executor DAG-step delegation |
| `bb29ee9` | Workspace-aware remote execution plans |
| `73f717a` | Remote auto-selection and executor-only boot profile behavior |
| `9c8eabb` | Removed legacy system fallback and moved to registry installs |
| `219e386` | Promoted official systems to installable extensions |
| `a6bfb36` | Public control-surface alignment around extension/brain/policy/service/remote |
| `e35cf1b` | Remote event streaming and system-tool extraction |
| `c74801c` | Remote daemon execution control plane |
| `920048d` | Kernel control plane and public taxonomy upgrade |

## What Is Implemented

### 1. Public control surface

The public noun-based CLI model exists:

- `task`
- `brain`
- `policy`
- `extension`
- `service`
- `remote`

Compatibility aliases remain where needed, but the public taxonomy is now
coherent.

### 2. Daemon profiles

One daemon binary supports:

- `desktop`
- `headless`

These are preset-driven runtime profiles rather than separate codebases. Profile
selection is surfaced in config and CLI.

### 3. Node identity

Node identity is no longer ad hoc remote metadata. The engine now persists:

- immutable `node_id` derived from the node public key
- renameable `node_name`
- Ed25519 signing keypair

This is the basis for peer trust and remote request signing.

### 4. Policy and approvals

`policy` is the active internal model. The repo also now has explicit approval
handling:

- approval modes: `default`, `allowlist`, `open`, `assisted` placeholder
- approval rules in TOML
- pending approval queue
- API and CLI controls for mode/rules

The non-overridable kernel floor still applies above approval mode.

### 5. Secrets

Vault-backed secret storage exists and is now the canonical persistent backend
surface. Desktop can still use compatibility backends, but the repo now has the
right abstraction for:

- `vault`
- `auto`
- `keychain`
- `env`

The intended UX is write-only: set, replace, delete, list status, and test.

### 6. Runtime and extensions

System tools are no longer treated as permanent built-in kernel behavior. The
runtime supports installable system extensions and keeps the registry/runtime
split intact.

### 7. Remote mesh

Remote is materially real:

- peer pairing and trust
- streamed remote task events
- remote executor delegation
- bundled remote execution plans
- signed remote requests
- replay protection
- node identity in remote state
- ZeroTier status/join surfaces for transport hints

### 8. Hosted shell and daemon auth

The repo now contains:

- local daemon auth/session layer
- password setup/login/lock/reauth
- authenticated WebSocket event stream
- hosted-shell WebUI client
- config, approvals, remote, settings, policy, and brains pages

### 9. Service install controls

The engine supports install/uninstall/status flows for:

- login service
- boot service

macOS and Linux surfaces are implemented at the service-file level in the core
repo.

## What Is Not Fully Done

### Cross-platform service finish

- Windows service install is still not implemented.
- The separate `app/` repo has an initial mac shell, but Windows/Linux app
  shells are still not built out.

### ZeroTier transport depth

- ZeroTier local Service API integration exists for status/join and publishing
  route hints.
- Managed-name sync and deeper transport orchestration are not finished.
- `libzt` is intentionally not used in this phase.

### Remote execution depth

- Remote executor delegation is real.
- Bundled plans are real.
- Signed requests and replay protection are real.
- Full arbitrary DAG graph-slice export is still not complete.

### Extension ecosystem polish

- Installable system extensions exist.
- A polished signed public catalog/discovery/update experience is still behind.

### Product polish

- The WebUI is now a truthful hosted shell, but some pages are still operator
  surfaces more than polished end-user UX.
- Cross-device consumer shells beyond the mac menu bar app are still missing.

## Repository Hygiene Snapshot

As of this review, the repo is not fully clean:

- tracked dirty file: `webui/tsconfig.json`
- long-standing untracked trees remain under:
  - `.github/`
  - `docs/`
  - `engine/examples/`
  - `engine/src/db/`
  - `engine/src/prompt/`
  - `engine/target/`
  - `schema/`
  - `scripts/`
  - `tests/`
  - multiple `tools/` subtrees
  - `website/`

Those were not auto-deleted in this review because they predate this cleanup
pass and may still contain user-owned work.

## Recommended Next Steps

1. Finish Windows service install support.
2. Continue remote execution depth toward fuller graph-slice delegation.
3. Improve ZeroTier transport beyond route-hint status/join.
4. Separate repo hygiene cleanup from architecture work so the large untracked
   trees can be reviewed intentionally.
5. Keep reducing legacy naming in remaining docs and compatibility surfaces.
