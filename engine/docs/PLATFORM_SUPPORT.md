# Platform-Specific Support

This document describes how Rove handles platform-specific differences across Linux, macOS, and Windows.

## Requirements

- **Requirement 25.2**: Use platform-specific paths (/ on Unix, \ on Windows)
- **Requirement 25.5**: Handle platform-specific line endings (LF on Unix, CRLF on Windows)

## Path Handling

### Automatic Platform-Specific Separators

Rove uses Rust's `std::path::Path` and `PathBuf` types throughout the codebase, which automatically handle platform-specific path separators:

- **Unix (Linux, macOS)**: Forward slash (`/`)
- **Windows**: Backslash (`\`)

This means that path operations work correctly on all platforms without manual string manipulation.

### Examples

```rust
use std::path::PathBuf;

// This works correctly on all platforms
let path = PathBuf::from("home").join("user").join("file.txt");

// On Unix: home/user/file.txt
// On Windows: home\user\file.txt
```

### Path Canonicalization

The `canonicalize()` method resolves paths to their absolute form using the platform-specific separator:

```rust
let canonical = path.canonicalize()?;
// On Unix: /home/user/workspace/file.txt
// On Windows: C:\Users\user\workspace\file.txt
```

### Modules Using Platform-Agnostic Paths

The following modules use `Path` and `PathBuf` for automatic platform compatibility:

- **`fs_guard`**: File system security with path validation
- **`config`**: Configuration management with path expansion
- **`runtime`**: Core tool and plugin loading
- **`daemon`**: PID file management

## Line Ending Handling

### Platform-Specific Line Endings

Different operating systems use different line ending conventions:

- **Unix (Linux, macOS)**: LF (`\n`)
- **Windows**: CRLF (`\r\n`)

### Line Ending Utilities

The `platform` module provides utilities for handling line endings:

```rust
use rove_engine::platform::{
    LINE_ENDING,
    normalize_line_endings,
    to_unix_line_endings,
    to_windows_line_endings,
};

// Platform-specific line ending constant
let line = format!("Hello{}", LINE_ENDING);
// On Unix: "Hello\n"
// On Windows: "Hello\r\n"

// Normalize to platform-specific format
let text = "line1\r\nline2\nline3\r\n";
let normalized = normalize_line_endings(text);
// On Unix: "line1\nline2\nline3\n"
// On Windows: "line1\r\nline2\r\nline3\r\n"

// Convert to specific format
let unix = to_unix_line_endings(text);     // Always LF
let windows = to_windows_line_endings(text); // Always CRLF
```

### When to Use Line Ending Utilities

Use line ending utilities when:

1. **Reading user-provided files**: Files may have mixed line endings
2. **Writing cross-platform files**: Ensure consistent format
3. **Generating output**: Use platform-specific line endings for user display
4. **Storing canonical format**: Use Unix line endings for internal storage

### File I/O Best Practices

```rust
use std::fs;
use rove_engine::platform::{normalize_line_endings, LINE_ENDING};

// Reading files - normalize line endings
let content = fs::read_to_string("file.txt")?;
let normalized = normalize_line_endings(&content);

// Writing files - use platform-specific line endings
let output = format!("line1{}line2{}", LINE_ENDING, LINE_ENDING);
fs::write("output.txt", output)?;
```

## Platform Detection

The `platform` module provides utilities for detecting the current platform:

```rust
use rove_engine::platform::{is_unix, is_windows, platform_name};

if is_unix() {
    println!("Running on Unix-like system");
}

if is_windows() {
    println!("Running on Windows");
}

let name = platform_name();
// Returns: "linux", "macos", "windows", or "unknown"
```

## Platform-Specific Code

Use `#[cfg]` attributes for platform-specific code:

```rust
#[cfg(unix)]
fn unix_specific_function() {
    // Unix-only code
}

#[cfg(windows)]
fn windows_specific_function() {
    // Windows-only code
}

#[cfg(target_os = "linux")]
fn linux_specific_function() {
    // Linux-only code
}

#[cfg(target_os = "macos")]
fn macos_specific_function() {
    // macOS-only code
}
```

### Examples in Codebase

- **`daemon/mod.rs`**: Signal handling (SIGTERM on Unix, different mechanism on Windows)
- **`fs_guard/mod.rs`**: Symlink tests (Unix-specific)

## Testing Platform-Specific Behavior

### Unit Tests

Platform-specific tests use `#[cfg]` attributes:

```rust
#[test]
fn test_platform_specific() {
    #[cfg(unix)]
    {
        assert_eq!(path_separator(), "/");
    }

    #[cfg(windows)]
    {
        assert_eq!(path_separator(), "\\");
    }
}
```

### Integration Tests

The `platform_integration_test.rs` file contains comprehensive tests for:

- Path separator handling
- Line ending normalization
- Path canonicalization
- Cross-platform compatibility
- Path traversal prevention

Run platform-specific tests:

```bash
# Run all platform tests
cargo test --package engineplatform

# Run integration tests
cargo test --package engine--test platform_integration_test
```

## CI/CD Platform Testing

**Requirement 25.7**: Test on all supported platforms in CI/CD

The CI/CD pipeline should test on:

- **Linux**: Ubuntu latest
- **macOS**: macOS latest
- **Windows**: Windows latest

Example GitHub Actions configuration:

```yaml
strategy:
  matrix:
    os: [ubuntu-latest, macos-latest, windows-latest]
runs-on: ${{ matrix.os }}
```

## Core Tool Loading

**Requirement 25.4**: Load Core_Tools with platform-specific extensions
**Requirement 25.6**: Use #[cfg(target_os)] for platform-specific code

Core tools are loaded with platform-specific extensions:

- **Linux**: `.so` (shared object)
- **macOS**: `.dylib` (dynamic library)
- **Windows**: `.dll` (dynamic link library)

### Automatic Extension Handling

The `runtime/native.rs` module uses `libloading` which automatically handles platform-specific shared library extensions. The `platform` module provides utilities for constructing library filenames:

```rust
use rove_engine::platform::{library_extension, library_prefix, library_filename};

// Get the platform-specific extension
let ext = library_extension();
// Linux: "so", macOS: "dylib", Windows: "dll"

// Get the platform-specific prefix
let prefix = library_prefix();
// Unix: "lib", Windows: ""

// Construct a full library filename
let filename = library_filename("telegram");
// Linux: "libtelegram.so"
// macOS: "libtelegram.dylib"
// Windows: "telegram.dll"
```

### Platform-Specific Code

The library loading utilities use `#[cfg(target_os)]` attributes for platform-specific behavior:

```rust
#[cfg(target_os = "linux")]
pub fn library_extension() -> &'static str {
    "so"
}

#[cfg(target_os = "macos")]
pub fn library_extension() -> &'static str {
    "dylib"
}

#[cfg(target_os = "windows")]
pub fn library_extension() -> &'static str {
    "dll"
}
```

### Manifest Configuration

When specifying core tools in the manifest, use platform-specific paths:

```json
{
  "core_tools": [
    {
      "name": "telegram",
      "version": "0.1.0",
      "path": "core-tools/telegram/target/release/libtelegram.so",
      "hash": "sha256:CCCC...",
      "signature": "ed25519:DDDD...",
      "platform": "linux-x86_64"
    },
    {
      "name": "telegram",
      "version": "0.1.0",
      "path": "core-tools/telegram/target/release/libtelegram.dylib",
      "hash": "sha256:EEEE...",
      "signature": "ed25519:FFFF...",
      "platform": "macos-aarch64"
    },
    {
      "name": "telegram",
      "version": "0.1.0",
      "path": "core-tools/telegram/target/release/telegram.dll",
      "hash": "sha256:GGGG...",
      "signature": "ed25519:HHHH...",
      "platform": "windows-x86_64"
    }
  ]
}
```

The manifest should include separate entries for each platform, with the correct file extension and path for that platform. The runtime will automatically select the appropriate entry based on the current platform.

### Building Core Tools for Multiple Platforms

When building core tools, ensure you generate the correct library format for each platform:

```bash
# Linux
cargo build --release --package telegram
# Produces: target/release/libtelegram.so

# macOS
cargo build --release --package telegram
# Produces: target/release/libtelegram.dylib

# Windows
cargo build --release --package telegram
# Produces: target/release/telegram.dll
```

The Rust compiler automatically generates the correct library format based on the target platform.

## Summary

Rove achieves cross-platform compatibility through:

1. **Automatic path handling**: Using `Path` and `PathBuf` for platform-agnostic paths
2. **Line ending utilities**: Normalizing and converting line endings as needed
3. **Platform detection**: Runtime detection for platform-specific behavior
4. **Conditional compilation**: Using `#[cfg]` for platform-specific code
5. **Comprehensive testing**: Testing on all supported platforms

This approach ensures that Rove works consistently across Linux, macOS, and Windows without requiring manual platform-specific code in most cases.
