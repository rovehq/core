use std::sync::Arc;

pub struct WorkspaceLocks {
    locks: Arc<dashmap::DashMap<std::path::PathBuf, Arc<tokio::sync::Mutex<()>>>>,
}

impl WorkspaceLocks {
    pub fn new() -> Self {
        Self {
            locks: Arc::new(dashmap::DashMap::new()),
        }
    }

    pub fn get_lock(&self, workspace: &std::path::Path) -> Arc<tokio::sync::Mutex<()>> {
        self.locks
            .entry(workspace.to_path_buf())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }
}

impl Default for WorkspaceLocks {
    fn default() -> Self {
        Self::new()
    }
}
