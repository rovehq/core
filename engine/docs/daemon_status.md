# Daemon Status Command

## Overview

The daemon status command provides comprehensive information about the Rove daemon's running state and the availability of LLM providers.

## Implementation

The status command is implemented in `src/daemon/mod.rs` as part of the `DaemonManager` struct.

### Status Information

The `DaemonStatus` struct contains:

1. **Daemon Running Status**
   - `is_running`: Boolean indicating if the daemon is currently running
   - `pid`: Process ID if the daemon is running (None otherwise)
   - `pid_file`: Path to the PID file

2. **Provider Availability**
   - `ollama`: Whether Ollama is available (checked via TCP connection)
   - `openai`: Whether OpenAI API key is configured in keychain
   - `anthropic`: Whether Anthropic API key is configured in keychain
   - `gemini`: Whether Gemini API key is configured in keychain
   - `nvidia_nim`: Whether NVIDIA NIM API key is configured in keychain

## Usage

```rust
use rove_engine::daemon::DaemonManager;
use rove_engine::config::Config;

let config = Config::load_or_create()?;
let status = DaemonManager::status(&config)?;

println!("Daemon running: {}", status.is_running);
if let Some(pid) = status.pid {
    println!("PID: {}", pid);
}

println!("\nProvider Availability:");
println!("  Ollama: {}", status.providers.ollama);
println!("  OpenAI: {}", status.providers.openai);
println!("  Anthropic: {}", status.providers.anthropic);
println!("  Gemini: {}", status.providers.gemini);
println!("  NVIDIA NIM: {}", status.providers.nvidia_nim);
```

## Provider Availability Checking

### Ollama

Ollama availability is checked by attempting to establish a TCP connection to the configured base URL (typically `http://localhost:11434`). The check:
- Parses the URL to extract host and port
- Attempts a TCP connection with a 2-second timeout
- Returns `true` if the connection succeeds, `false` otherwise

This approach is lightweight and doesn't require the full HTTP client stack.

### Cloud Providers

Cloud provider availability is determined by checking if API keys exist in the OS keychain:
- Uses the `SecretManager::has_secret()` method
- Non-interactive check (doesn't prompt for missing keys)
- Returns `true` if the key exists in the keychain, `false` otherwise

## Requirements Validation

This implementation validates **Requirement 14.13**:
> WHEN the user runs `Rove status`, THE Engine SHALL report Daemon status and provider availability

The status command reports:
1. Whether the daemon is running (via PID file check)
2. The daemon's process ID if running
3. Availability status for all supported LLM providers

## Testing

The implementation includes comprehensive tests:

1. **test_daemon_status**: Verifies basic status reporting (running/not running, PID)
2. **test_daemon_status_provider_availability**: Verifies provider availability checking works without errors

All tests pass without prompting for user input, ensuring the status check is fully automated.

## Future Enhancements

Potential improvements for future iterations:

1. **Enhanced Ollama Check**: Use HTTP request to `/api/version` endpoint for more reliable detection
2. **Provider Health Check**: Not just availability, but also health/responsiveness
3. **Model Availability**: Check which models are available for each provider
4. **Rate Limit Status**: Show current rate limit usage
5. **Last Activity**: Show when the daemon last processed a task
