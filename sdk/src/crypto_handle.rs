use crate::errors::EngineError;
use std::sync::Arc;

#[derive(Clone)]
pub struct CryptoHandle {
    inner: Arc<dyn CryptoHandleImpl>,
}

impl CryptoHandle {
    pub fn new(inner: Arc<dyn CryptoHandleImpl>) -> Self {
        Self { inner }
    }

    pub fn sign_data(&self, data: &[u8]) -> Result<Vec<u8>, EngineError> {
        self.inner.sign_data(data)
    }

    pub fn verify_signature(&self, data: &[u8], signature: &[u8]) -> Result<(), EngineError> {
        self.inner.verify_signature(data, signature)
    }

    pub fn get_secret(&self, key: &str) -> Result<String, EngineError> {
        self.inner.get_secret(key)
    }

    pub fn scrub_secrets(&self, text: &str) -> String {
        self.inner.scrub_secrets(text)
    }
}

pub trait CryptoHandleImpl: Send + Sync {
    fn sign_data(&self, data: &[u8]) -> Result<Vec<u8>, EngineError>;
    fn verify_signature(&self, data: &[u8], signature: &[u8]) -> Result<(), EngineError>;
    fn get_secret(&self, key: &str) -> Result<String, EngineError>;
    fn scrub_secrets(&self, text: &str) -> String;
}
