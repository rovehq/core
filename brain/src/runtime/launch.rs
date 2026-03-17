use super::discover::model_exists;
use super::handle::ReasoningBrain;

impl ReasoningBrain {
    pub fn load_model(&mut self, path: &str) -> Result<(), String> {
        if !model_exists(path) {
            return Err("Model not found. Falling back to cloud.".to_string());
        }

        self.active_model = Some(path.to_string());
        Ok(())
    }
}
