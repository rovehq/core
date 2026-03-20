//! Error types and handling
//!
//! This module provides the error types used throughout the Rove engine.
//! All errors implement the `RoveErrorExt` trait which provides user-friendly
//! hints and indicates whether errors are recoverable.
//!
//! # Security
//!
//! All error messages are scrubbed to ensure:
//! - No secrets (API keys, tokens) are included
//! - No file paths are exposed to remote users
//! - All messages are safe to display to end users

use thiserror::Error;

/// Trait for Rove error extensions
///
/// This trait provides additional context for errors, including user-friendly
/// hints and recoverability information. All engine errors implement this trait.
pub trait RoveErrorExt {
    /// Returns a user-friendly hint for the error
    ///
    /// The hint is safe to display to end users and does not contain:
    /// - Secrets (API keys, tokens, passwords)
    /// - File paths (for remote users)
    /// - Internal implementation details
    fn user_hint(&self) -> &str;

    /// Returns whether the error is recoverable
    ///
    /// Recoverable errors can be retried or worked around. Non-recoverable
    /// errors typically require manual intervention or system restart.
    fn is_recoverable(&self) -> bool;
}

/// Main engine error type
///
/// This enum represents all possible errors that can occur in the Rove engine.
/// Each variant includes context-specific information while ensuring no sensitive
/// data (secrets, internal paths) is exposed in error messages.
///
/// # Error Categories
///
/// - **Configuration**: Invalid or missing configuration
/// - **Database**: SQLite operation failures
/// - **LLM Provider**: API failures, authentication errors
/// - **Plugin**: WASM plugin execution errors
/// - **File System**: Path validation and access errors
/// - **Security**: Cryptographic verification failures
/// - **Rate Limiting**: Operation throttling
///
/// # Examples
///
/// ```
/// use sdk::errors::{EngineError, RoveErrorExt};
/// use std::path::PathBuf;
///
/// let error = EngineError::PathDenied(PathBuf::from(".ssh"));
/// println!("Hint: {}", error.user_hint());
/// assert!(error.is_recoverable());
///
/// let fatal_error = EngineError::InvalidSignature;
/// assert!(!fatal_error.is_recoverable());
/// ```
#[derive(Debug, Error)]
pub enum EngineError {
    // Configuration errors
    #[error("Configuration error: {0}")]
    Config(String),

    // Database errors
    #[error("Database error: {0}")]
    Database(String),

    // LLM provider errors
    #[error("LLM provider error: {0}")]
    LLMProvider(String),

    // Plugin errors
    #[error("Plugin error: {0}")]
    Plugin(String),

    #[error("Plugin not in manifest: {0}")]
    PluginNotInManifest(String),

    #[error("Plugin not loaded: {0}")]
    PluginNotLoaded(String),

    // File system security errors
    #[error("Path denied: {0:?}")]
    PathDenied(std::path::PathBuf),

    #[error("Path outside workspace: {0:?}")]
    PathOutsideWorkspace(std::path::PathBuf),

    #[error("Path canonicalization failed for {0:?}: {1}")]
    PathCanonicalization(std::path::PathBuf, String),

    // Daemon errors
    #[error("Daemon already running")]
    DaemonAlreadyRunning,

    // LLM routing errors
    #[error("All LLM providers exhausted")]
    AllProvidersExhausted,

    // Agent loop errors
    #[error("Operation aborted by user")]
    OperationAbortedByUser,

    #[error("Infinite loop detected: {0}")]
    InfiniteLoopDetected(String),

    #[error("Max iterations exceeded")]
    MaxIterationsExceeded,

    #[error("LLM call timed out")]
    LLMTimeout,

    #[error("Result size exceeded: {size} bytes > {limit} bytes")]
    ResultSizeExceeded { size: usize, limit: usize },

    // Tool errors
    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    #[error("Tool not in manifest: {0}")]
    ToolNotInManifest(String),

    #[error("Tool not loaded: {0}")]
    ToolNotLoaded(String),

    #[error("Tool not permitted: {0}")]
    ToolNotPermitted(String),

    #[error("Tool error: {0}")]
    ToolError(String),

    #[error("Route unavailable: {0}")]
    RouteUnavailable(String),

    // Security errors
    #[error("Invalid signature")]
    InvalidSignature,

    #[error("Hash mismatch")]
    HashMismatch(String),

    #[error("Envelope expired")]
    EnvelopeExpired,

    #[error("Nonce reused")]
    NonceReused,

    #[error("Command not allowed")]
    CommandNotAllowed(String),

    #[error("Shell injection attempt detected")]
    ShellInjectionAttempt,

    #[error("Shell metacharacters detected")]
    ShellMetacharactersDetected(String),

    #[error("Dangerous pipe pattern detected")]
    DangerousPipeDetected,

    // Rate limiting errors
    #[error("Rate limit exceeded for {src} (Tier {tier}): {count}/{limit} operations in {window}")]
    RateLimitExceeded {
        src: String,
        tier: i32,
        count: i64,
        limit: i64,
        window: String,
    },

    #[error("Circuit breaker tripped for {src}: {count} operations in 60 seconds")]
    CircuitBreakerTripped { src: String, count: i64 },

    // Keyring errors
    #[error("Keyring error: {0}")]
    KeyringError(String),

    // Network errors
    #[error("Network error: {0}")]
    Network(String),

    // Library loading errors
    #[error("Library load failed: {0}")]
    LibraryLoadFailed(String),

    #[error("Symbol not found: {0}")]
    SymbolNotFound(String),

    // Operation errors
    #[error("Unknown operation: {0}")]
    UnknownOperation(String),

    #[error("Write query not allowed")]
    WriteQueryNotAllowed,

    // Generic IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl RoveErrorExt for EngineError {
    fn user_hint(&self) -> &str {
        match self {
            // Configuration errors
            Self::Config(_) => "Check your config.toml file for errors",

            // Database errors
            Self::Database(_) => "Database operation failed. Try restarting the daemon",

            // LLM provider errors
            Self::LLMProvider(_) => "LLM provider unavailable. Check your API keys and network",
            Self::AllProvidersExhausted => "No LLM providers available. Check configuration",

            // Plugin errors
            Self::Plugin(_) => "Plugin execution failed. Check plugin logs",
            Self::PluginNotInManifest(_) => "Plugin not found in manifest. Check installation",
            Self::PluginNotLoaded(_) => "Plugin not loaded. Try restarting the daemon",

            // File system security errors
            Self::PathDenied(_) => "Access to this path is not allowed",
            Self::PathOutsideWorkspace(_) => "Operation must be within workspace",
            Self::PathCanonicalization(_, _) => "Invalid path specified",

            // Daemon errors
            Self::DaemonAlreadyRunning => "Stop the existing daemon first with 'rove stop'",

            // Agent loop errors
            Self::OperationAbortedByUser => "Operation was cancelled",
            Self::InfiniteLoopDetected(_) => {
                "Agent repeated the same action too many times. Task aborted"
            }
            Self::MaxIterationsExceeded => "Task too complex. Try breaking it into smaller steps",
            Self::LLMTimeout => "LLM provider took too long to respond. Try again",
            Self::ResultSizeExceeded { .. } => "Result too large. Try a more specific query",

            // Tool errors
            Self::ToolNotFound(_) => "The requested tool is not available",
            Self::ToolNotInManifest(_) => "Tool not found in manifest. Check installation",
            Self::ToolNotLoaded(_) => "Tool not loaded. Try restarting the daemon",
            Self::ToolNotPermitted(_) => "This tool is not permitted for the current agent",
            Self::ToolError(_) => "Tool operation failed",
            Self::RouteUnavailable(_) => "The requested execution route is not available",

            // Security errors
            Self::InvalidSignature => "Security verification failed. File may be tampered",
            Self::HashMismatch(_) => "Security verification failed. File may be corrupted",
            Self::EnvelopeExpired => "Request expired. Please try again",
            Self::NonceReused => "Duplicate request detected. Please try again",
            Self::CommandNotAllowed(_) => "This command is not permitted",
            Self::ShellInjectionAttempt => "Command rejected for security reasons",
            Self::ShellMetacharactersDetected(_) => "Command contains unsafe characters",
            Self::DangerousPipeDetected => "Command contains dangerous patterns",

            // Rate limiting errors
            Self::RateLimitExceeded { .. } => {
                "Rate limit exceeded. Please wait before trying again"
            }
            Self::CircuitBreakerTripped { .. } => "Too many operations. Manual unlock required",

            // Keyring errors
            Self::KeyringError(_) => "Failed to access secure storage. Check system keychain",

            // Network errors
            Self::Network(_) => "Network operation failed. Check your connection",

            // Library loading errors
            Self::LibraryLoadFailed(_) => "Failed to load system component",
            Self::SymbolNotFound(_) => "System component is incompatible",

            // Operation errors
            Self::UnknownOperation(_) => "Unknown operation requested",
            Self::WriteQueryNotAllowed => "Write operations not permitted",

            // Generic IO error
            Self::Io(_) => "File system operation failed",
        }
    }

    fn is_recoverable(&self) -> bool {
        match self {
            // Non-recoverable errors
            Self::DaemonAlreadyRunning
            | Self::OperationAbortedByUser
            | Self::InfiniteLoopDetected(_)
            | Self::AllProvidersExhausted
            | Self::InvalidSignature
            | Self::HashMismatch(_)
            | Self::CircuitBreakerTripped { .. }
            | Self::LibraryLoadFailed(_)
            | Self::SymbolNotFound(_) => false,

            // All other errors are potentially recoverable
            _ => true,
        }
    }
}
