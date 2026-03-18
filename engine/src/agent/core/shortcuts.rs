use std::time::{Duration, Instant};

use anyhow::Result;
use regex::Regex;
use sdk::TaskDomain;

use super::{AgentCore, TaskResult};
use crate::gateway::Task;

impl AgentCore {
    pub(super) async fn try_shortcut_task(
        &mut self,
        task_id: &uuid::Uuid,
        task: &Task,
    ) -> Result<Option<TaskResult>> {
        if let Some(result) = self.try_wait_shortcut(task_id, task).await? {
            return Ok(Some(result));
        }

        if let Some(result) = self.try_time_shortcut(task_id, task).await? {
            return Ok(Some(result));
        }

        if let Some(result) = self.try_memory_maintenance_shortcut(task_id, task).await? {
            return Ok(Some(result));
        }

        Ok(None)
    }

    async fn try_wait_shortcut(
        &self,
        task_id: &uuid::Uuid,
        task: &Task,
    ) -> Result<Option<TaskResult>> {
        let pattern = Regex::new(r"(?i)^\s*wait\s+(\d+)\s+seconds?\s+then\s+say\s+(.+?)\s*$")?;
        let Some(captures) = pattern.captures(task.input.trim()) else {
            return Ok(None);
        };

        let seconds = captures
            .get(1)
            .and_then(|value| value.as_str().parse::<u64>().ok())
            .unwrap_or(0);
        let answer = captures
            .get(2)
            .map(|value| value.as_str().trim().trim_matches('"').to_string())
            .unwrap_or_default();
        let started = Instant::now();

        self.insert_user_event(task_id, &task.input, "general")
            .await?;
        tokio::time::sleep(Duration::from_secs(seconds)).await;
        self.insert_answer_event(task_id, &answer, 1, "general")
            .await?;

        Ok(Some(TaskResult::success(
            task_id.to_string(),
            answer,
            "shortcut".to_string(),
            started.elapsed().as_millis() as i64,
            1,
            TaskDomain::General,
            false,
        )))
    }

    async fn try_time_shortcut(
        &self,
        task_id: &uuid::Uuid,
        task: &Task,
    ) -> Result<Option<TaskResult>> {
        let input = task.input.to_ascii_lowercase();
        let matches_time = input.contains("what time is it")
            || input.contains("current time")
            || input.trim() == "time";
        if !matches_time {
            return Ok(None);
        }

        let answer = chrono::Local::now()
            .format("It is %Y-%m-%d %H:%M:%S %Z.")
            .to_string();
        self.insert_user_event(task_id, &task.input, "general")
            .await?;
        self.insert_answer_event(task_id, &answer, 1, "general")
            .await?;

        Ok(Some(TaskResult::success(
            task_id.to_string(),
            answer,
            "shortcut".to_string(),
            0,
            1,
            TaskDomain::General,
            false,
        )))
    }

    async fn try_memory_maintenance_shortcut(
        &self,
        task_id: &uuid::Uuid,
        task: &Task,
    ) -> Result<Option<TaskResult>> {
        let normalized = task.input.trim().to_ascii_lowercase();
        if normalized != "trigger memory consolidation" && normalized != "run memory consolidation"
        {
            return Ok(None);
        }

        let Some(memory_system) = self.memory_system() else {
            return Ok(None);
        };

        self.insert_user_event(task_id, &task.input, "general")
            .await?;
        let result = memory_system.consolidate().await?;
        let answer = match result {
            crate::conductor::types::ConsolidationResult::Skipped { reason } => {
                format!("Memory consolidation skipped: {}", reason)
            }
            crate::conductor::types::ConsolidationResult::Completed {
                memories_processed,
                insights_generated,
            } => format!(
                "Memory consolidation completed: {} memories processed, {} insights generated.",
                memories_processed, insights_generated
            ),
        };
        self.insert_answer_event(task_id, &answer, 1, "general")
            .await?;

        Ok(Some(TaskResult::success(
            task_id.to_string(),
            answer,
            "shortcut".to_string(),
            0,
            1,
            TaskDomain::General,
            false,
        )))
    }

    pub(super) async fn try_git_commit_shortcut(
        &mut self,
        task_id: &uuid::Uuid,
        task: &Task,
        domain_str: &str,
        domain: TaskDomain,
        sensitive: bool,
        start_time: Instant,
    ) -> Result<Option<TaskResult>> {
        if !looks_like_git_commit_request(&task.input) {
            return Ok(None);
        }

        self.execute_scripted_command(task_id, 1, domain_str, "git status --short")
            .await?;
        let status_output = self.latest_observation(task_id, 1).await?;
        if status_output.trim().is_empty() {
            let answer = "There are no changes to commit.".to_string();
            self.insert_answer_event(task_id, &answer, 1, domain_str)
                .await?;
            return Ok(Some(TaskResult::success(
                task_id.to_string(),
                answer,
                "shortcut".to_string(),
                start_time.elapsed().as_millis() as i64,
                1,
                domain,
                sensitive,
            )));
        }

        self.execute_scripted_command(task_id, 1, domain_str, "git add -A")
            .await?;
        let commit_message = generate_commit_message(&status_output);
        let commit_command = format!("git commit -m {}", shlex::try_quote(&commit_message)?);
        self.execute_scripted_command(task_id, 1, domain_str, &commit_command)
            .await?;
        let commit_output = self.latest_observation(task_id, 1).await?;
        let answer = if commit_output.trim().is_empty() {
            format!("Committed your changes with message: {}.", commit_message)
        } else {
            format!(
                "Committed your changes with message `{}`.\n\n{}",
                commit_message,
                commit_output.trim()
            )
        };
        self.insert_answer_event(task_id, &answer, 1, domain_str)
            .await?;

        Ok(Some(TaskResult::success(
            task_id.to_string(),
            answer,
            "shortcut".to_string(),
            start_time.elapsed().as_millis() as i64,
            1,
            domain,
            sensitive,
        )))
    }

    async fn latest_observation(&self, task_id: &uuid::Uuid, iteration: usize) -> Result<String> {
        let events = self
            .task_repo
            .get_agent_events(&task_id.to_string())
            .await?;
        let step_num = (iteration * 2) as i64;
        Ok(events
            .into_iter()
            .rfind(|event| event.event_type == "observation" && event.step_num == step_num)
            .and_then(|event| serde_json::from_str::<serde_json::Value>(&event.payload).ok())
            .and_then(|payload| {
                payload
                    .get("observation")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_default())
    }
}

fn looks_like_git_commit_request(input: &str) -> bool {
    let input_lower = input.to_ascii_lowercase();
    input_lower.contains("commit my changes")
}

fn generate_commit_message(status_output: &str) -> String {
    let mut files = Vec::new();
    for line in status_output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(path) = trimmed.split_whitespace().last() {
            files.push(path.to_string());
        }
    }

    match files.as_slice() {
        [] => "update workspace".to_string(),
        [single] => format!("update {}", file_stem_label(single)),
        [first, second, ..] => format!(
            "update {} and {}",
            file_stem_label(first),
            file_stem_label(second)
        ),
    }
}

fn file_stem_label(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.replace(['_', '-'], " "))
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| "files".to_string())
}
