use anyhow::Result;
use serde::Serialize;

use crate::channels::manager::ChannelManager;
use crate::config::Config;
use crate::remote::RemoteManager;
use crate::security::{password_protection_state, PasswordProtectionState};
use crate::storage::Database;
use crate::system::health::RuntimeHealthSnapshot;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OnboardingStepState {
    Complete,
    ActionRequired,
}

#[derive(Debug, Clone, Serialize)]
pub struct OnboardingStep {
    pub id: String,
    pub title: String,
    pub state: OnboardingStepState,
    pub summary: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct OnboardingChecklist {
    pub completed_steps: usize,
    pub total_steps: usize,
    pub steps: Vec<OnboardingStep>,
}

pub async fn collect(
    config: &Config,
    db: &Database,
    health: &RuntimeHealthSnapshot,
) -> Result<OnboardingChecklist> {
    let recent_tasks = db.tasks().get_recent_tasks(1).await?;
    let has_recent_task = !recent_tasks.is_empty();

    let telegram = ChannelManager::new(config.clone())
        .telegram_status()
        .await?;
    let remote_status = RemoteManager::new(config.clone()).status().ok();
    let paired_nodes = remote_status
        .as_ref()
        .map(|status| status.paired_nodes)
        .unwrap_or(0);
    let transport_count = remote_status
        .as_ref()
        .map(|status| status.transports.len())
        .unwrap_or(0);

    let steps = vec![
        OnboardingStep {
            id: "install_truth".to_string(),
            title: "Install truth".to_string(),
            state: if health.initialized
                && health.config_file.writable
                && health.data_dir.writable
                && health.database.writable
            {
                OnboardingStepState::Complete
            } else {
                OnboardingStepState::ActionRequired
            },
            summary: if health.initialized {
                format!(
                    "Config, data directory, and database exist for profile `{}`.",
                    health.profile
                )
            } else {
                "Config, data directory, or database is still missing.".to_string()
            },
            action: "Run `rove init` and then `rove doctor` until config, data, and database are ready.".to_string(),
        },
        OnboardingStep {
            id: "auth_truth".to_string(),
            title: "Auth truth".to_string(),
            state: match password_protection_state(config).unwrap_or(PasswordProtectionState::LegacyUnsealed) {
                PasswordProtectionState::Sealed => OnboardingStepState::Complete,
                PasswordProtectionState::LegacyUnsealed
                | PasswordProtectionState::Tampered
                | PasswordProtectionState::Uninitialized => OnboardingStepState::ActionRequired,
            },
            summary: match password_protection_state(config).unwrap_or(PasswordProtectionState::LegacyUnsealed) {
                PasswordProtectionState::Sealed => {
                    "Local daemon password is configured and device-sealed.".to_string()
                }
                PasswordProtectionState::LegacyUnsealed => {
                    "Local daemon password exists but is not device-sealed yet.".to_string()
                }
                PasswordProtectionState::Tampered => {
                    "Local daemon password integrity failed and must be reset.".to_string()
                }
                PasswordProtectionState::Uninitialized => {
                    "Local daemon password is not configured yet.".to_string()
                }
            },
            action: "Run `rove auth reset-password` to harden or recover daemon auth, or complete the initial password setup flow.".to_string(),
        },
        OnboardingStep {
            id: "daemon_running".to_string(),
            title: "Daemon running".to_string(),
            state: if health.daemon_running {
                OnboardingStepState::Complete
            } else {
                OnboardingStepState::ActionRequired
            },
            summary: if health.daemon_running {
                health
                    .daemon_pid
                    .map(|pid| format!("Daemon is running with pid {pid}."))
                    .unwrap_or_else(|| "Daemon is running.".to_string())
            } else {
                "Daemon is not running.".to_string()
            },
            action: "Run `rove start` and confirm the CLI, WebUI, and logs agree on runtime state.".to_string(),
        },
        OnboardingStep {
            id: "first_task".to_string(),
            title: "First task".to_string(),
            state: if has_recent_task {
                OnboardingStepState::Complete
            } else {
                OnboardingStepState::ActionRequired
            },
            summary: if has_recent_task {
                "At least one task has already been executed through the runtime.".to_string()
            } else {
                "No completed task history was found yet.".to_string()
            },
            action: "Run a small local task first before adding channels or remotes.".to_string(),
        },
        OnboardingStep {
            id: "first_channel".to_string(),
            title: "First channel".to_string(),
            state: if telegram.can_receive {
                OnboardingStepState::Complete
            } else {
                OnboardingStepState::ActionRequired
            },
            summary: if telegram.can_receive {
                format!(
                    "Telegram is ready and bound to {}.",
                    telegram
                        .default_agent_name
                        .as_deref()
                        .unwrap_or("the default handler agent")
                )
            } else if telegram.enabled {
                "Telegram is enabled but still needs setup before it can receive tasks.".to_string()
            } else {
                "No production-ready channel is configured yet.".to_string()
            },
            action: telegram
                .doctor
                .first()
                .cloned()
                .unwrap_or_else(|| "Configure Telegram with `rove channel telegram setup ...` and enable it once the local runtime is healthy.".to_string()),
        },
        OnboardingStep {
            id: "first_remote".to_string(),
            title: "First remote".to_string(),
            state: if paired_nodes > 0 {
                OnboardingStepState::Complete
            } else {
                OnboardingStepState::ActionRequired
            },
            summary: if paired_nodes > 0 {
                format!(
                    "{paired_nodes} paired remote node(s) are available through {transport_count} transport(s)."
                )
            } else if transport_count > 0 {
                format!(
                    "{transport_count} transport(s) are configured, but no paired remote nodes are ready yet."
                )
            } else {
                "No paired remote nodes are configured yet.".to_string()
            },
            action: "Pair another node from the remote surface, then verify trust and transport state before dispatching work remotely.".to_string(),
        },
    ];

    let completed_steps = steps
        .iter()
        .filter(|step| step.state == OnboardingStepState::Complete)
        .count();
    let total_steps = steps.len();

    Ok(OnboardingChecklist {
        completed_steps,
        total_steps,
        steps,
    })
}

pub fn print_text(checklist: &OnboardingChecklist) {
    println!("First-Run Checklist:");
    println!();
    for step in &checklist.steps {
        let marker = match step.state {
            OnboardingStepState::Complete => 'x',
            OnboardingStepState::ActionRequired => ' ',
        };
        println!("  [{marker}] {} — {}", step.title, step.summary);
        if step.state == OnboardingStepState::ActionRequired {
            println!("      next: {}", step.action);
        }
    }
    println!();
    println!(
        "Progress: {}/{} steps complete.",
        checklist.completed_steps, checklist.total_steps
    );
}
