# Rove Core Repo Cleanup Report

Date: March 23, 2026

## Scope Of This Cleanup Pass

This pass focused on high-signal cleanup:

- correcting stale architecture and product docs
- aligning READMEs with the current runtime shape
- recording a realistic implementation-status report
- documenting the repo hygiene debt that still exists

It did not blindly delete old directories or unrelated untracked trees.

## Docs Corrected In This Pass

- top-level workspace overview
- engine runtime overview
- brain runtime overview
- SDK overview
- hosted-shell WebUI overview
- developer guide index
- high-level architecture diagram
- implementation-status report
- repo-cleanup report

## Review-Graph Baseline

From `code-review-graph status`:

- Nodes: 3423
- Edges: 31209
- Files: 326
- Languages: rust, python, typescript, javascript, tsx

This confirms the repo is large enough that stale docs turn into real
coordination problems. The cleanup therefore prioritized architectural accuracy
over broad cosmetic churn.

## Observed Hygiene Issues

### 1. Dirty worktree

Tracked dirty file:

- `webui/tsconfig.json`

This file was already dirty before the current doc cleanup. It was not touched
here.

### 2. Large untracked surface

Current untracked paths include:

- `.github/`
- `docs/`
- `engine/examples/`
- `engine/src/db/`
- `engine/src/prompt/`
- `engine/target/`
- `schema/`
- `scripts/`
- `test_local_brain.sh`
- `tests/`
- `tools/README.md`
- `tools/api_docs.html`
- `tools/app-launcher/`
- `tools/file-watcher/`
- `tools/notification/`
- `tools/remote/`
- `tools/telegram/`
- `tools/ui-server/`
- `website/`

These were left intact because they are not obviously safe to delete without a
separate ownership pass.

### 3. Documentation drift

Before this pass, the most visible drift was:

- root README still said web surfaces were intentionally deferred
- engine README still described `steering/` rather than the current `policy`
  model
- WebUI README still described the daemon as the primary UI host
- developer guide still described older JSON-RPC/REPL assumptions

Those were corrected here.

## Cleanup Recommendations

### Safe next cleanup

- audit the tracked `webui/tsconfig.json` diff and either commit or revert it
  intentionally
- review the untracked `docs/` tree now that it has a deliberate purpose
- decide whether `engine/examples/`, `schema/`, `scripts/`, and `website/` are
  meant to be revived or archived

### Do not do blindly

- do not mass-delete untracked directories without checking ownership
- do not collapse compatibility shims until downstream callers are verified
- do not treat graph size as proof that a directory is active or inactive

## Result

The codebase is better documented and easier to reason about after this pass,
but the repository is still not fully clean. The remaining dirt is now
explicitly documented instead of hidden behind outdated READMEs.
