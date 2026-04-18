//! Platform-specific utilities
//!
//! This module provides utilities for handling platform-specific differences
//! in file paths, line endings, and other OS-specific behaviors.
//!
//! # Path Handling
//!
//! Rust's `std::path::Path` and `PathBuf` automatically handle platform-specific
//! path separators (/ on Unix, \ on Windows). This module provides additional
//! utilities for working with paths in a cross-platform manner.
//!
//! # Line Endings
//!
//! Different operating systems use different line ending conventions:
//! - Unix (Linux, macOS): LF (\n)
//! - Windows: CRLF (\r\n)
//!
//! This module provides utilities for normalizing and converting line endings
//! when reading and writing files.
//!
//! # Platform-Specific Functions
//!
//! The following functions are provided by platform-specific modules:
//! - `default_transport_path()` - Brain communication transport path
//! - `llama_search_paths()` - Known llama-server installation locations
//! - `available_ram()` - Available system RAM in bytes
//! - `cpu_load_percent()` - Approximate CPU load percentage
//! - `keychain_get()` - Get secret from OS keychain
//! - `keychain_set()` - Set secret in OS keychain
//!
//! # Requirements
//!
//! - Requirement 25.2: Use platform-specific paths (/ on Unix, \ on Windows)
//! - Requirement 25.5: Handle platform-specific line endings (LF on Unix, CRLF on Windows)

pub mod libraries;
pub mod lines;
pub mod paths;

// Platform-specific modules (compile-time dispatch)
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::*;

pub use libraries::*;
pub use lines::*;
pub use paths::*;
