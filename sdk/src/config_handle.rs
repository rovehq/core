use std::sync::Arc;

#[derive(Clone)]
pub struct ConfigHandle {
    inner: Arc<dyn ConfigHandleImpl>,
}

impl ConfigHandle {
    pub fn new(inner: Arc<dyn ConfigHandleImpl>) -> Self {
        Self { inner }
    }

    pub fn get(&self, key: &str) -> Option<serde_json::Value> {
        self.inner.get(key)
    }

    pub fn get_string(&self, key: &str) -> Option<String> {
        self.get(key).and_then(|v| v.as_str().map(String::from))
    }

    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(|v| v.as_i64())
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.get(key).and_then(|v| v.as_bool())
    }
}

pub trait ConfigHandleImpl: Send + Sync {
    fn get(&self, key: &str) -> Option<serde_json::Value>;
}
