use crate::errors::EngineError;
use std::sync::Arc;

#[derive(Clone)]
pub struct DbHandle {
    inner: Arc<dyn DbHandleImpl>,
}

impl DbHandle {
    pub fn new(inner: Arc<dyn DbHandleImpl>) -> Self {
        Self { inner }
    }

    pub fn query(
        &self,
        sql: &str,
        params: Vec<serde_json::Value>,
    ) -> Result<Vec<serde_json::Value>, EngineError> {
        self.inner.query(sql, params)
    }
}

pub trait DbHandleImpl: Send + Sync {
    fn query(
        &self,
        sql: &str,
        params: Vec<serde_json::Value>,
    ) -> Result<Vec<serde_json::Value>, EngineError>;
}
