use crate::errors::EngineError;
use std::sync::Arc;

#[derive(Clone)]
pub struct BusHandle {
    inner: Arc<dyn BusHandleImpl>,
}

impl BusHandle {
    pub fn new(inner: Arc<dyn BusHandleImpl>) -> Self {
        Self { inner }
    }

    pub fn subscribe(&self, event_type: &str) -> Result<(), EngineError> {
        self.inner.subscribe(event_type)
    }

    pub fn publish(&self, event_type: &str, payload: serde_json::Value) -> Result<(), EngineError> {
        self.inner.publish(event_type, payload)
    }
}

pub trait BusHandleImpl: Send + Sync {
    fn subscribe(&self, event_type: &str) -> Result<(), EngineError>;
    fn publish(&self, event_type: &str, payload: serde_json::Value) -> Result<(), EngineError>;
}
