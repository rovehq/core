pub struct AdapterRegistry {
    pub active_adapter: Option<String>,
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl AdapterRegistry {
    pub fn new() -> Self {
        AdapterRegistry {
            active_adapter: None,
        }
    }

    pub fn set_adapter(&mut self, lora_path: &str) -> Result<(), String> {
        // Fast swap (<50ms)
        self.active_adapter = Some(lora_path.to_string());
        Ok(())
    }
}
