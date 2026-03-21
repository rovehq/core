# Plugin Authoring And Registry Flow

This document is the shortest path from "I want to build a plugin" to "Rove can install it from a registry."

## Mental model

Rove treats plugins as versioned packages with three layers:

- `manifest.json`
  - declares the stable contract: name, version, plugin type, permissions, trust tier, SDK version
- `plugin-package.json`
  - declares the release payload: artifact path, runtime config path, payload hash, payload signature
- `runtime.json`
  - declares the tool catalog Rove should register before the plugin is ever loaded

The engine only executes installed plugins. Authoring, testing, packing, publishing, and installing all converge on that package shape.

## Fast author loop

1. Create a scaffold:
   - `rove plugin new my-pdf-reader --type skill`
2. Run unit tests:
   - `cargo test`
3. Run the local mock-runtime loop:
   - `rove plugin test ./my-pdf-reader --input "read this PDF and summarise it"`
4. Build the release artifact:
   - `cargo build --target wasm32-wasip1 --release`
5. Replace placeholder hash and signature values in `plugin-package.json`
6. Sign `manifest.json`

If you need a reference package, start from:

- [/Users/as/workspace/rove/core/engine/tests/fixtures/plugins/echo-skill](/Users/as/workspace/rove/core/engine/tests/fixtures/plugins/echo-skill)

## Local package install

Install directly from a package directory:

```bash
rove plugin install ./my-pdf-reader
```

This path verifies:

- manifest signature
- payload hash
- payload signature
- SDK compatibility
- plugin-type constraints
- permission review

## Distribution bundle flow

Create a normalized release bundle:

```bash
rove plugin pack ./my-pdf-reader
```

That produces a directory containing:

- `manifest.json`
- `plugin-package.json`
- `runtime.json`
- the normalized artifact file
- `release.json`
- optional `README.md`

The release bundle is the portable unit for publishing.

## Static registry flow

Publish into a static registry directory:

```bash
rove plugin publish ./my-pdf-reader --registry-dir ./registry
```

Publishing writes:

- `registry.json`
  - top-level catalog of plugin ids and latest versions
- `<plugin-id>/index.json`
  - version index for one plugin
- `<plugin-id>/<version>/...`
  - the release bundle for that version

This layout is designed for simple file hosting:

- local filesystem
- shared network directory
- static object storage
- Git-backed pages or releases

## Registry install flow

Install from a registry directory or URL:

```bash
rove plugin install my-pdf-reader --registry ./registry --version 0.1.0
rove plugin install my-pdf-reader --registry https://example.com/rove-registry
```

Rove resolves:

1. `<registry>/<plugin-id>/index.json`
2. the requested version, or the latest version
3. the referenced release bundle files
4. the normal install verification path

So a registry install is still a verified package install. The transport changes; the trust checks do not.

## Recommended repository layout for plugin authors

```text
my-plugin/
  Cargo.toml
  manifest.json
  plugin-package.json
  runtime.json
  src/lib.rs
  tests/integration.rs
  README.md
```

For a shared plugin registry repo, keep this layout:

```text
registry/
  registry.json
  my-plugin/
    index.json
    0.1.0/
      manifest.json
      plugin-package.json
      runtime.json
      my_plugin.wasm
      release.json
      README.md
```

## What to keep stable

These are the public packaging contracts:

- the manifest fields
- the package file fields
- the runtime tool catalog shape
- the registry index files

If those change casually, plugin authors lose trust in the platform. Treat them as versioned contracts.
