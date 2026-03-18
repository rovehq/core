use chrono::Utc;
use sdk::TaskSource;
use std::path::PathBuf;
use uuid::Uuid;

use crate::risk_assessor::RiskTier;

#[derive(Debug, Clone)]
pub struct Task {
    pub id: Uuid,
    pub input: String,
    pub source: TaskSource,
    pub risk_tier_override: Option<RiskTier>,
    pub session_id: Option<Uuid>,
    pub workspace: Option<PathBuf>,
    pub created_at: i64,
}

impl Task {
    pub fn build_from_cli(input: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            input: input.into(),
            source: TaskSource::Cli,
            risk_tier_override: None,
            session_id: None,
            workspace: std::env::current_dir().ok(),
            created_at: Utc::now().timestamp(),
        }
    }

    pub fn build_from_telegram(input: impl Into<String>, session_id: Option<Uuid>) -> Self {
        Self {
            id: Uuid::new_v4(),
            input: input.into(),
            source: TaskSource::Telegram(String::new()),
            risk_tier_override: None,
            session_id,
            workspace: None,
            created_at: Utc::now().timestamp(),
        }
    }

    pub fn build_from_websocket(input: impl Into<String>, session_id: Option<Uuid>) -> Self {
        Self {
            id: Uuid::new_v4(),
            input: input.into(),
            source: TaskSource::WebUI,
            risk_tier_override: None,
            session_id,
            workspace: None,
            created_at: Utc::now().timestamp(),
        }
    }
}

impl From<TaskSource> for crate::risk_assessor::OperationSource {
    fn from(source: TaskSource) -> Self {
        match source {
            TaskSource::Cli => crate::risk_assessor::OperationSource::Local,
            TaskSource::Telegram(_) | TaskSource::WebUI | TaskSource::Remote(_) => {
                crate::risk_assessor::OperationSource::Remote
            }
        }
    }
}
