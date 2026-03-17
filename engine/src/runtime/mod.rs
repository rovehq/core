//! Runtime module for loading and managing core tools and plugins
//!
//! This module provides two runtime implementations:
//! - NativeRuntime: Loads core tools as native shared libraries with four-gate verification
//! - WasmRuntime: Loads plugins as WASM modules with two-gate verification

pub mod native;
pub mod wasm;

pub use native::NativeRuntime;
pub use wasm::WasmRuntime;
