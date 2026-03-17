use crate::errors::EngineError;
use std::sync::Arc;

#[derive(Clone)]
pub struct NetworkHandle {
    inner: Arc<dyn NetworkHandleImpl>,
}

impl NetworkHandle {
    pub fn new(inner: Arc<dyn NetworkHandleImpl>) -> Self {
        Self { inner }
    }

    pub fn http_get(&self, url: &str) -> Result<Vec<u8>, EngineError> {
        self.inner.http_get(url)
    }

    pub fn http_post(&self, url: &str, body: Vec<u8>) -> Result<Vec<u8>, EngineError> {
        self.inner.http_post(url, body)
    }
}

pub trait NetworkHandleImpl: Send + Sync {
    fn http_get(&self, url: &str) -> Result<Vec<u8>, EngineError>;
    fn http_post(&self, url: &str, body: Vec<u8>) -> Result<Vec<u8>, EngineError>;
}
