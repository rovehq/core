use anyhow::Result;
use serde_json::json;

use crate::storage::{AgentEvent, PendingTaskStatus};

use super::output::{OutputFormat, TaskView};

#[derive(Debug, Clone, PartialEq)]
pub struct DispatchSummary {
    pub domain_label: String,
    pub complexity: String,
    pub sensitive: bool,
    pub confidence: Option<f32>,
    pub route: Option<String>,
}

impl DispatchSummary {
    pub fn new(
        domain_label: impl Into<String>,
        complexity: impl Into<String>,
        sensitive: bool,
        confidence: Option<f32>,
        route: Option<String>,
    ) -> Self {
        Self {
            domain_label: domain_label.into(),
            complexity: complexity.into(),
            sensitive,
            confidence,
            route,
        }
    }

    fn inline_summary(&self) -> String {
        format!(
            "{} · {} · {}",
            self.domain_label,
            self.complexity,
            if self.sensitive {
                "sensitive"
            } else {
                "normal"
            }
        )
    }
}

pub fn print_start(
    task: &str,
    task_id: &str,
    format: OutputFormat,
    view: TaskView,
    dispatch: Option<&DispatchSummary>,
) -> Result<()> {
    match format {
        OutputFormat::Text => {
            for line in render_start(task, task_id, view, dispatch) {
                println!("{line}");
            }
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "status": "running",
                    "task_id": task_id,
                    "task": task,
                    "dispatch": dispatch_payload(dispatch),
                    "view": view.as_str(),
                }))?
            );
        }
    }

    Ok(())
}

pub fn print_status_change(
    status: PendingTaskStatus,
    format: OutputFormat,
    view: TaskView,
) -> Result<()> {
    match format {
        OutputFormat::Text => {
            if let Some(line) = render_status_change(status, view) {
                println!("{line}");
            }
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "status": "progress",
                    "phase": status_label(status),
                    "view": view.as_str(),
                }))?
            );
        }
    }

    Ok(())
}

pub fn print_success(
    task_id: &str,
    answer: &str,
    provider_used: &str,
    duration_ms: i64,
    iterations: usize,
    format: OutputFormat,
    view: TaskView,
    dispatch: Option<&DispatchSummary>,
) -> Result<()> {
    match format {
        OutputFormat::Text => {
            for line in render_success(
                task_id,
                answer,
                provider_used,
                duration_ms,
                iterations,
                view,
                dispatch,
            ) {
                println!("{line}");
            }
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "status": "completed",
                    "task_id": task_id,
                    "answer": answer,
                    "provider": provider_used,
                    "duration_ms": duration_ms,
                    "iterations": iterations,
                    "dispatch": dispatch_payload(dispatch),
                    "view": view.as_str(),
                }))?
            );
        }
    }

    Ok(())
}

pub fn print_failure(error: &anyhow::Error, format: OutputFormat, view: TaskView) -> Result<()> {
    match format {
        OutputFormat::Text => {
            for line in render_failure(&error.to_string(), view) {
                println!("{line}");
            }
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "status": "failed",
                    "error": error.to_string(),
                    "view": view.as_str(),
                }))?
            );
        }
    }

    Ok(())
}

pub fn print_stream_event(event: &AgentEvent, view: TaskView) {
    if let Some(line) = render_event(event, view) {
        println!("{line}");
    }
}

fn dispatch_payload(dispatch: Option<&DispatchSummary>) -> serde_json::Value {
    match dispatch {
        Some(dispatch) => json!({
            "domain": dispatch.domain_label,
            "complexity": dispatch.complexity,
            "sensitive": dispatch.sensitive,
            "confidence": dispatch.confidence,
            "route": dispatch.route,
        }),
        None => serde_json::Value::Null,
    }
}

fn render_start(
    task: &str,
    task_id: &str,
    view: TaskView,
    dispatch: Option<&DispatchSummary>,
) -> Vec<String> {
    match view {
        TaskView::Gist => vec![format!("Started task {task_id}")],
        TaskView::Logs => {
            let mut lines = vec![
                format!("[start] task_id={task_id}"),
                format!("[prompt] {task}"),
            ];
            if let Some(dispatch) = dispatch {
                lines.push(format!(
                    "[dispatch] domain={} confidence={} complexity={} sensitive={} route={}",
                    dispatch.domain_label,
                    dispatch
                        .confidence
                        .map(|value| format!("{value:.2}"))
                        .unwrap_or_else(|| "-".to_string()),
                    dispatch.complexity,
                    dispatch.sensitive,
                    dispatch.route.as_deref().unwrap_or("-"),
                ));
            }
            lines.push("[status] started".to_string());
            lines
        }
        TaskView::Clean | TaskView::Live => {
            let mut lines = vec![format!("Started task {task_id}"), format!("Prompt: {task}")];
            if let Some(dispatch) = dispatch {
                lines.push(format!("Dispatch: {}", dispatch.inline_summary()));
            }
            if matches!(view, TaskView::Live) {
                lines.push("Status: running".to_string());
            }
            lines.push(String::new());
            lines
        }
    }
}

fn render_status_change(status: PendingTaskStatus, view: TaskView) -> Option<String> {
    match view {
        TaskView::Live => Some(format!("Status: {}", status_label(status))),
        TaskView::Logs => Some(format!("[status] {}", status_label(status))),
        TaskView::Clean | TaskView::Gist => None,
    }
}

fn render_success(
    task_id: &str,
    answer: &str,
    provider_used: &str,
    duration_ms: i64,
    iterations: usize,
    view: TaskView,
    dispatch: Option<&DispatchSummary>,
) -> Vec<String> {
    match view {
        TaskView::Gist => vec![answer.to_string()],
        TaskView::Logs => {
            let mut lines = vec![format!(
                "[result] task_id={task_id} provider={provider_used} duration_ms={duration_ms} iterations={iterations}"
            )];
            if let Some(dispatch) = dispatch {
                lines.push(format!(
                    "[dispatch] domain={} complexity={} sensitive={}",
                    dispatch.domain_label, dispatch.complexity, dispatch.sensitive
                ));
            }
            lines.push("[answer]".to_string());
            lines.push(answer.to_string());
            lines
        }
        TaskView::Clean | TaskView::Live => {
            let mut lines = vec![
                "Answer".to_string(),
                answer.to_string(),
                String::new(),
                format!(
                    "Summary: {provider_used} · {duration_ms}ms · {}",
                    iteration_label(iterations)
                ),
            ];
            if let Some(dispatch) = dispatch {
                lines.push(format!("Classified: {}", dispatch.inline_summary()));
            }
            lines.push(format!("Task ID: {task_id}"));
            lines
        }
    }
}

fn render_failure(error: &str, view: TaskView) -> Vec<String> {
    match view {
        TaskView::Gist => vec![format!("Task failed: {error}")],
        TaskView::Logs => vec!["[result] failed".to_string(), format!("[error] {error}")],
        TaskView::Clean | TaskView::Live => vec![format!("Task failed: {error}")],
    }
}

fn render_event(event: &AgentEvent, view: TaskView) -> Option<String> {
    match view {
        TaskView::Clean | TaskView::Gist => None,
        TaskView::Live => match event.event_type.as_str() {
            "thought" => Some(format!("Plan: {}", summarize_line(&event.payload))),
            "tool_call" => Some(format!("Tool: {}", summarize_line(&event.payload))),
            "observation" => Some(format!("Observation: {}", summarize_line(&event.payload))),
            "error" => Some(format!("Error: {}", summarize_line(&event.payload))),
            _ => None,
        },
        TaskView::Logs => Some(format!(
            "[event:{}] {}",
            event.event_type,
            event.payload.replace('\n', "\\n")
        )),
    }
}

fn summarize_line(text: &str) -> String {
    let single_line = text.replace('\n', " ");
    if single_line.len() > 120 {
        format!("{}...", &single_line[..117])
    } else {
        single_line
    }
}

fn iteration_label(iterations: usize) -> String {
    if iterations == 1 {
        "1 iteration".to_string()
    } else {
        format!("{iterations} iterations")
    }
}

fn status_label(status: PendingTaskStatus) -> &'static str {
    match status {
        PendingTaskStatus::Pending => "queued",
        PendingTaskStatus::Running => "running",
        PendingTaskStatus::Done => "completed",
        PendingTaskStatus::Failed => "failed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::PendingTaskStatus;

    #[test]
    fn clean_start_renders_dispatch_summary() {
        let dispatch = DispatchSummary::new("general", "simple", false, Some(0.72), None);
        let lines = render_start(
            "what is the current time",
            "task-123",
            TaskView::Clean,
            Some(&dispatch),
        );

        assert_eq!(lines[0], "Started task task-123");
        assert!(lines
            .iter()
            .any(|line| line == "Dispatch: general · simple · normal"));
    }

    #[test]
    fn logs_view_keeps_confidence_and_route() {
        let dispatch =
            DispatchSummary::new("git", "medium", true, Some(0.91), Some("local".to_string()));
        let lines = render_start(
            "commit my changes",
            "task-123",
            TaskView::Logs,
            Some(&dispatch),
        );

        assert!(lines.iter().any(|line| line.contains("confidence=0.91")));
        assert!(lines.iter().any(|line| line.contains("route=local")));
    }

    #[test]
    fn live_status_uses_human_labels() {
        assert_eq!(
            render_status_change(PendingTaskStatus::Pending, TaskView::Live).as_deref(),
            Some("Status: queued")
        );
    }
}
