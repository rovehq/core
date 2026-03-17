//! Native runtime for loading and managing core tools.

mod call;
mod load;
#[cfg(test)]
mod tests;
mod verify;

use crate::crypto::CryptoModule;
use sdk::{core_tool::CoreTool, manifest::Manifest};
use std::collections::HashMap;
use std::sync::Arc;

pub struct NativeRuntime {
    tools: HashMap<String, Box<dyn CoreTool>>,
    manifest: Manifest,
    crypto: Arc<CryptoModule>,
    #[allow(dead_code)]
    libraries: HashMap<String, libloading::Library>,
}

impl NativeRuntime {
    pub fn new(manifest: Manifest, crypto: Arc<CryptoModule>) -> Self {
        tracing::info!("Initializing NativeRuntime");
        Self {
            tools: HashMap::new(),
            manifest,
            crypto,
            libraries: HashMap::new(),
        }
    }
}
