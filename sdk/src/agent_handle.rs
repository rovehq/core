use crate::errors::EngineError;
use std::sync::Arc;

#[derive(Clone)]
pub struct AgentHandle {
    inner: Arc<dyn AgentHandleImpl>,
}

impl AgentHandle {
    pub fn new(inner: Arc<dyn AgentHandleImpl>) -> Self {
        Self { inner }
    }

    pub fn submit_task(&self, task_input: String) -> Result<String, EngineError> {
        self.inner.submit_task(task_input)
    }

    pub fn get_task_status(&self, task_id: &str) -> Result<String, EngineError> {
        self.inner.get_task_status(task_id)
    }
}

pub trait AgentHandleImpl: Send + Sync {
    fn submit_task(&self, task_input: String) -> Result<String, EngineError>;
    fn get_task_status(&self, task_id: &str) -> Result<String, EngineError>;
}
