/// Final task result returned by the agent loop.
#[derive(Debug, Clone)]
pub struct TaskResult {
    pub task_id: String,
    pub answer: String,
    pub provider_used: String,
    pub duration_ms: i64,
    pub iterations: usize,
}

impl TaskResult {
    pub fn success(
        task_id: String,
        answer: String,
        provider_used: String,
        duration_ms: i64,
        iterations: usize,
    ) -> Self {
        Self {
            task_id,
            answer,
            provider_used,
            duration_ms,
            iterations,
        }
    }
}
