use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub task_id: String,
    pub tool_name: String,
    pub risk_tier: u8,
    pub summary: String,
    pub created_at: i64,
    pub auto_resolve_after_secs: Option<u64>,
}

type ApprovalWaiter = oneshot::Sender<bool>;

fn pending_approvals() -> &'static DashMap<String, ApprovalRequest> {
    static MAP: OnceLock<DashMap<String, ApprovalRequest>> = OnceLock::new();
    MAP.get_or_init(DashMap::new)
}

fn approval_waiters() -> &'static DashMap<String, ApprovalWaiter> {
    static MAP: OnceLock<DashMap<String, ApprovalWaiter>> = OnceLock::new();
    MAP.get_or_init(DashMap::new)
}

pub async fn request_approval(
    task_id: &str,
    tool_name: &str,
    risk_tier: u8,
    summary: impl Into<String>,
    timeout: Option<Duration>,
    default_on_timeout: bool,
) -> bool {
    let id = Uuid::new_v4().to_string();
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or_default();

    let approval = ApprovalRequest {
        id: id.clone(),
        task_id: task_id.to_string(),
        tool_name: tool_name.to_string(),
        risk_tier,
        summary: summary.into(),
        created_at,
        auto_resolve_after_secs: timeout.map(|value| value.as_secs()),
    };

    let (tx, rx) = oneshot::channel();
    pending_approvals().insert(id.clone(), approval);
    approval_waiters().insert(id.clone(), tx);

    let resolved = match timeout {
        Some(timeout) => tokio::time::timeout(timeout, rx)
            .await
            .ok()
            .and_then(Result::ok),
        None => rx.await.ok(),
    };

    pending_approvals().remove(&id);
    approval_waiters().remove(&id);
    resolved.unwrap_or(default_on_timeout)
}

pub fn list_pending() -> Vec<ApprovalRequest> {
    let mut values = pending_approvals()
        .iter()
        .map(|entry| entry.value().clone())
        .collect::<Vec<_>>();
    values.sort_by_key(|approval| approval.created_at);
    values
}

pub fn resolve(id: &str, approved: bool) -> bool {
    let waiter = approval_waiters().remove(id).map(|(_, waiter)| waiter);
    pending_approvals().remove(id);
    if let Some(waiter) = waiter {
        let _ = waiter.send(approved);
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn request_approval_tracks_pending_and_resolves() {
        let approval_task = tokio::spawn(async {
            request_approval(
                "task-1",
                "write_file",
                2,
                "Allow write_file for task-1",
                None,
                false,
            )
            .await
        });

        let approval = loop {
            if let Some(approval) = list_pending().into_iter().find(|value| value.task_id == "task-1")
            {
                break approval;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };

        assert_eq!(approval.tool_name, "write_file");
        assert!(resolve(&approval.id, true));
        assert!(approval_task.await.expect("approval task"));
        assert!(list_pending()
            .into_iter()
            .all(|value| value.id != approval.id));
    }

    #[tokio::test]
    async fn request_approval_honors_timeout_default() {
        let approved = request_approval(
            "task-timeout",
            "run_command",
            1,
            "Allow run_command for task-timeout",
            Some(Duration::from_millis(50)),
            true,
        )
        .await;

        assert!(approved);
    }
}
