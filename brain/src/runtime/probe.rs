use super::handle::ReasoningBrain;

impl ReasoningBrain {
    pub fn check_ram_before_load(free_ram_mb: u64) -> Result<(), String> {
        if free_ram_mb < 2000 {
            return Err(
                "Insufficient RAM (< 2000MB) for reasoning brain. Falling back to cloud."
                    .to_string(),
            );
        }
        Ok(())
    }
}
