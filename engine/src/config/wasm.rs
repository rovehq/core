use serde::{Deserialize, Serialize};

use super::defaults::{default_wasm_fuel_limit, default_wasm_max_memory_mb, default_wasm_timeout_secs};

/// Operator-tunable WASM plugin runtime defaults.
///
/// These values act as the default ceiling for every loaded WASM plugin.
/// A plugin's manifest entry or `.capabilities.json` sidecar may only make
/// limits stricter (never looser).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WasmConfig {
    #[serde(default = "default_wasm_max_memory_mb")]
    pub default_max_memory_mb: u32,
    #[serde(default = "default_wasm_timeout_secs")]
    pub default_timeout_secs: u64,
    #[serde(default = "default_wasm_fuel_limit")]
    pub default_fuel_limit: u64,
}

impl Default for WasmConfig {
    fn default() -> Self {
        Self {
            default_max_memory_mb: default_wasm_max_memory_mb(),
            default_timeout_secs: default_wasm_timeout_secs(),
            default_fuel_limit: default_wasm_fuel_limit(),
        }
    }
}

impl WasmConfig {
    /// Clamp fields to safe ranges after deserialization.
    pub fn normalize(&mut self) {
        self.default_max_memory_mb = self.default_max_memory_mb.clamp(1, 1024);
        self.default_timeout_secs = self.default_timeout_secs.clamp(1, 600);
        self.default_fuel_limit = self.default_fuel_limit.max(1);
    }
}
