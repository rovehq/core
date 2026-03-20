# MCP Packages

Rove treats MCP as an optional connector runtime.
Users only install the servers they need, and Rove only starts them on first use.

This document covers the local package format used by:

- `rove mcp install /path/to/package`
- `rove mcp upgrade /path/to/package`
- `rove mcp scaffold /path/to/new-package ...`

## What Lives Where

- `runtime/mcp/`
  - Rove connecting to external MCP servers
  - process spawn, sandbox, lifecycle, tool calls
- `api/mcp/`
  - Rove exposed outward as an MCP server
  - external MCP clients connecting to Rove

These are opposite directions. Do not mix them.

## Package Layout

An MCP package is a directory with:

```text
my-mcp-package/
  manifest.json
  plugin-package.json
  runtime.json
  README.md
```

There is no binary artifact for MCP packages.
`runtime.json` is the payload that gets hashed and signed.

## Files

### `manifest.json`

Describes the plugin contract:

- package name and version
- SDK version
- plugin type: `Mcp`
- trust tier
- declared permissions
- description
- manifest signature

Example shape:

```json
{
  "name": "GitHub MCP",
  "version": "0.1.0",
  "sdk_version": "0.1.0",
  "plugin_type": "Mcp",
  "permissions": {
    "filesystem": [],
    "network": ["api.github.com"],
    "memory_read": false,
    "memory_write": false,
    "tools": []
  },
  "trust_tier": "Community",
  "min_model": null,
  "description": "GitHub connector for Rove",
  "signature": "<manifest signature>"
}
```

### `plugin-package.json`

Install metadata:

- stable package id
- runtime config file path
- SHA256 of `runtime.json`
- signature of `runtime.json`
- enabled-by-default flag

Example:

```json
{
  "id": "github-mcp",
  "runtime_config": "runtime.json",
  "payload_hash": "<sha256 of runtime.json>",
  "payload_signature": "<signature of runtime.json>",
  "enabled": true
}
```

### `runtime.json`

The actual MCP runtime config consumed by Rove.
This maps directly onto `McpServerConfig`.

Example:

```json
{
  "name": "github",
  "template": "github",
  "description": "GitHub MCP",
  "command": "github-mcp-server",
  "args": ["stdio"],
  "profile": {
    "allow_network": true,
    "read_paths": [],
    "write_paths": [],
    "allow_tmp": true
  },
  "cached_tools": [],
  "enabled": true
}
```

## Scaffold Flow

Generate a package skeleton:

```bash
rove mcp scaffold ./github-mcp \
  --name "GitHub MCP" \
  --template github \
  --command github-mcp-server \
  --arg stdio
```

This creates:

- `manifest.json` with placeholder signature
- `plugin-package.json` with placeholder payload hash and signature
- `runtime.json`
- `README.md` with next steps

## Export Flow

Turn an existing configured server into a package skeleton:

```bash
rove mcp export github ./github-mcp
```

Optional custom manifest name:

```bash
rove mcp export github ./github-mcp --package-name "GitHub MCP"
```

This is useful when:

- you already proved a server config locally with `rove mcp add ...`
- you want to turn that working config into a shareable package
- you want to seed an official or community catalog entry from a real setup

The export command clears `cached_tools` and forces `enabled = true` in the
generated `runtime.json` so the package is clean before signing.

## Install Flow

1. Edit `runtime.json` until the command, args, and sandbox are correct.
2. Replace placeholder permissions in `manifest.json` with explicit paths and domains.
3. Compute SHA256 for `runtime.json` and place it in `plugin-package.json`.
4. Sign `runtime.json` and place the signature in `plugin-package.json`.
5. Sign `manifest.json` and replace the manifest signature.
6. Install with:

```bash
rove mcp install /path/to/package
```

Upgrade with:

```bash
rove mcp upgrade /path/to/package
```

## Server Sources

Rove merges two MCP sources into one inventory:

- config-backed servers from `config.toml`
- installed MCP packages from `installed_plugins`

When names collide, the installed MCP package wins.

`rove mcp list`, `show`, `enable`, `disable`, `remove`, `test`, and `tools`
work across both sources.

## Recommended Catalog Layout

For a shared MCP catalog repo, use one repo with clear separation:

```text
rove-mcp/
  templates/
    official/
    community/
  packages/
    official/
    community/
  README.md
```

Recommended split:

- `templates/`
  - lightweight presets and metadata
- `packages/official/`
  - Rove-maintained installable MCP packages
- `packages/community/`
  - contributed packages or examples

Keep `core` limited to:

- runtime manager
- MCP runtime host/client
- CLI/package handling
- security and sandbox enforcement

Do not put the package catalog inside `core`.
