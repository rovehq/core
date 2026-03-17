use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStatus {
    pub is_loaded: bool,
    pub active_model: Option<String>,
}

pub struct ReasoningBrain {
    pub active_model: Option<String>,
}

impl ReasoningBrain {
    pub fn new() -> Result<Self, String> {
        Ok(Self { active_model: None })
    }

    pub fn status(&self) -> RuntimeStatus {
        RuntimeStatus {
            is_loaded: self.active_model.is_some(),
            active_model: self.active_model.clone(),
        }
    }
}
