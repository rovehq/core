mod discover;
mod handle;
mod launch;
mod probe;

pub use handle::{ReasoningBrain, RuntimeStatus};

#[cfg(test)]
mod tests {
    use super::ReasoningBrain;

    #[test]
    fn test_ram_check_prevents_load_on_constrained_device() {
        let result = ReasoningBrain::check_ram_before_load(1500);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "Insufficient RAM (< 2000MB) for reasoning brain. Falling back to cloud."
        );
    }

    #[test]
    fn test_ram_check_allows_load_on_capable_device() {
        let result = ReasoningBrain::check_ram_before_load(3000);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cloud_fallback_on_missing_model() {
        let mut brain = ReasoningBrain::new().unwrap();
        let result = brain.load_model("/path/to/nonexistent/model.gguf");
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "Model not found. Falling back to cloud."
        );
    }
}
