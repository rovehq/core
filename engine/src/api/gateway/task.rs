use chrono::Utc;
use sdk::{RunContextId, RunIsolation, RunMode, TaskExecutionProfile, TaskSource};
use std::path::PathBuf;
use uuid::Uuid;

use crate::risk_assessor::RiskTier;

#[derive(Debug, Clone)]
pub struct Task {
    pub id: Uuid,
    pub input: String,
    pub source: TaskSource,
    pub execution_profile: Option<TaskExecutionProfile>,
    pub risk_tier_override: Option<RiskTier>,
    pub run_context_id: RunContextId,
    pub run_mode: RunMode,
    pub run_isolation: RunIsolation,
    pub session_id: Option<Uuid>,
    pub workspace: Option<PathBuf>,
    pub created_at: i64,
}

impl Task {
    pub fn build_from_cli(input: impl Into<String>) -> Self {
        Self::build_from_cli_with_context(
            input,
            std::env::current_dir().ok(),
            RunMode::Serial,
            RunIsolation::None,
        )
    }

    pub fn build_from_cli_with_context(
        input: impl Into<String>,
        workspace: Option<PathBuf>,
        run_mode: RunMode,
        run_isolation: RunIsolation,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            input: input.into(),
            source: TaskSource::Cli,
            execution_profile: None,
            risk_tier_override: None,
            run_context_id: RunContextId(Uuid::new_v4().to_string()),
            run_mode,
            run_isolation,
            session_id: None,
            workspace,
            created_at: Utc::now().timestamp(),
        }
    }

    pub fn build_from_telegram(input: impl Into<String>, session_id: Option<Uuid>) -> Self {
        Self {
            id: Uuid::new_v4(),
            input: input.into(),
            source: TaskSource::Telegram(String::new()),
            execution_profile: None,
            risk_tier_override: None,
            run_context_id: RunContextId(Uuid::new_v4().to_string()),
            run_mode: RunMode::Serial,
            run_isolation: RunIsolation::None,
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
            execution_profile: None,
            risk_tier_override: None,
            run_context_id: RunContextId(Uuid::new_v4().to_string()),
            run_mode: RunMode::Serial,
            run_isolation: RunIsolation::None,
            session_id,
            workspace: None,
            created_at: Utc::now().timestamp(),
        }
    }

    pub fn with_execution_profile(mut self, execution_profile: TaskExecutionProfile) -> Self {
        self.execution_profile = Some(execution_profile);
        self
    }

    pub fn with_id(mut self, id: Uuid) -> Self {
        self.id = id;
        self
    }
}

impl From<TaskSource> for crate::risk_assessor::OperationSource {
    fn from(source: TaskSource) -> Self {
        match source {
            TaskSource::Cli | TaskSource::Subagent(_) => {
                crate::risk_assessor::OperationSource::Local
            }
            TaskSource::Telegram(_)
            | TaskSource::Channel(_)
            | TaskSource::WebUI
            | TaskSource::Remote(_) => crate::risk_assessor::OperationSource::Remote,
        }
    }
}
