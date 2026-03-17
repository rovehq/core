use serde::{Deserialize, Serialize};
use sysinfo::System;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceProfile {
    pub total_ram_mb: u64,
    pub free_ram_mb: u64,
    pub recommended_brain: String,
}

pub fn get_device_profile(profile: &str) -> DeviceProfile {
    let mut sys = System::new_all();
    sys.refresh_memory();

    // sysinfo returns in bytes
    let total_ram_mb = sys.total_memory() / 1024 / 1024;
    let free_ram_mb = sys.free_memory() / 1024 / 1024;

    let recommended_brain = recommend_brain(free_ram_mb, profile).to_string();

    DeviceProfile {
        total_ram_mb,
        free_ram_mb,
        recommended_brain,
    }
}

pub fn recommend_brain(free_ram_mb: u64, profile: &str) -> &str {
    match (free_ram_mb, profile) {
        (r, _) if r < 800 => "none",
        (r, "code") if r < 2000 => "qwen2.5-coder-0.5b",
        (r, "code") if r >= 2000 => "qwen2.5-coder-1.5b",
        (r, "general") if r < 2000 => "qwen2.5-0.5b",
        (r, "general") if r >= 2000 => "llama3.2-1b",
        (r, _) if r >= 5000 => "phi-3.5-mini",
        _ => "qwen2.5-coder-0.5b",
    }
}
