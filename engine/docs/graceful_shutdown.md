# Graceful Shutdown Implementation

## Overview

The Rove daemon implements a comprehensive graceful shutdown sequence to ensure all components are properly cleaned up when the daemon stops. This document describes the implementation and how it satisfies the requirements.

## Requirements Satisfied

- **14.5**: Handle SIGTERM signal
- **14.6**: Set shutdown flag to refuse new tasks
- **14.7**: Wait up to 30 seconds for in-progress tasks
- **14.8**: Call stop() on all core tools
- **14.9**: Close all plugins
- **14.10**: Flush SQLite WAL
- **14.11**: Remove PID file
- **14.12**: Complete shutdown sequence

## Architecture

### Signal Handler (Requirement 14.5)

The daemon sets up a SIGTERM signal handler during startup:

```rust
let shutdown_flag = Arc::clone(&self.shutdown_flag);
let _signal_handle = Self::setup_signal_handler(shutdown_flag);
```

On Unix systems, this uses `tokio::signal::unix::signal` to listen for SIGTERM. When received, it sets the shutdown flag atomically.

### Shutdown Sequence

The `graceful_shutdown()` method implements the complete shutdown sequence:

#### 1. Set Shutdown Flag (Requirement 14.6)

```rust
self.signal_shutdown();
```

This sets an atomic boolean flag that can be checked by other components to refuse new tasks.

#### 2. Wait for In-Progress Tasks (Requirement 14.7, 14.8)

```rust
match self.wait_for_shutdown(Duration::from_secs(30)).await {
    Ok(_) => tracing::info!("All in-progress tasks completed"),
    Err(_) => tracing::warn!("Timeout waiting for tasks - proceeding with shutdown"),
}
```

The daemon waits up to 30 seconds for tasks to complete. If the timeout expires, it proceeds with shutdown anyway.

#### 3. Stop Core Tools (Requirement 14.9)

```rust
if let Some(native_runtime) = &self.native_runtime {
    let mut runtime = native_runtime.lock().await;
    runtime.unload_all();
}
```

Calls `stop()` on all loaded core tools through the `NativeRuntime::unload_all()` method.

#### 4. Close Plugins (Requirement 14.10)

```rust
if let Some(wasm_runtime) = &self.wasm_runtime {
    let mut runtime = wasm_runtime.lock().await;
    runtime.unload_all();
}
```

Unloads all WASM plugins through the `WasmRuntime::unload_all()` method.

#### 5. Flush SQLite WAL (Requirement 14.11)

```rust
if let Some(database) = &self.database {
    match database.flush_wal().await {
        Ok(_) => tracing::info!("SQLite WAL flushed successfully"),
        Err(e) => tracing::error!("Failed to flush SQLite WAL: {}", e),
    }
}
```

Ensures all pending database writes are persisted to disk by checkpointing the WAL.

#### 6. Remove PID File (Requirement 14.12)

```rust
if self.pid_file.exists() {
    match fs::remove_file(&self.pid_file) {
        Ok(_) => tracing::info!("PID file removed successfully"),
        Err(e) => tracing::error!("Failed to remove PID file: {}", e),
    }
}
```

Cleans up the PID file to indicate the daemon is no longer running.

## Usage

### Starting the Daemon

```rust
let config = Config::load_or_create()?;
let mut manager = DaemonManager::new(&config)?;

// Initialize components
manager.set_native_runtime(native_runtime);
manager.set_wasm_runtime(wasm_runtime);
manager.set_database(database);

// Start daemon (sets up signal handler)
manager.start().await?;
```

### Stopping the Daemon

From the CLI:

```bash
Rove stop
```

This sends SIGTERM to the daemon process, which triggers the graceful shutdown sequence.

Programmatically:

```rust
DaemonManager::stop(&config).await?;
```

### Manual Shutdown

For testing or manual control:

```rust
manager.graceful_shutdown(&config).await?;
```

## Error Handling

The shutdown sequence is designed to be resilient:

- Each step logs its status (info/error level)
- Errors in one step don't prevent subsequent steps
- Timeout on task waiting doesn't abort shutdown
- Missing components (not initialized) are handled gracefully

## Testing

Comprehensive integration tests verify each requirement:

- `test_signal_handler_setup`: Verifies SIGTERM handler setup
- `test_shutdown_flag_set`: Verifies flag is set
- `test_wait_for_tasks_timeout`: Verifies timeout behavior
- `test_native_runtime_shutdown`: Verifies core tools are stopped
- `test_wasm_runtime_shutdown`: Verifies plugins are closed
- `test_database_wal_flush`: Verifies WAL is flushed
- `test_pid_file_removal`: Verifies PID file cleanup
- `test_complete_shutdown_sequence`: End-to-end integration test

Run tests with:

```bash
cargo test --package engine--test daemon_graceful_shutdown_test
```

## Platform Support

### Unix (Linux, macOS)

Full support for SIGTERM signal handling using `nix` and `tokio::signal::unix`.

### Windows

Signal handler is a placeholder. Windows doesn't have SIGTERM, so a different mechanism (e.g., named events or service control) would be needed for production use.

## Future Enhancements

1. **Task Tracking**: Currently, the daemon waits for a fixed timeout. Future versions could track active tasks and wait only as long as needed.

2. **Graceful Degradation**: Could implement progressive shutdown levels (e.g., stop accepting new tasks but allow existing ones more time).

3. **Windows Support**: Implement proper Windows service control for graceful shutdown.

4. **Shutdown Hooks**: Allow components to register custom shutdown handlers.

## Related Documentation

- [Daemon Lifecycle Management](./daemon_lifecycle.md)
- [PID File Handling](./pid_file_handling.md)
- [Database WAL Management](./database_wal.md)
