use anyhow::Result;
use sdk::{SubagentRole, SubagentSpec, TaskExecutionProfile, WorkerPreset};

pub fn list_worker_presets() -> Vec<WorkerPreset> {
    vec![
        WorkerPreset {
            id: "researcher".to_string(),
            name: "Researcher".to_string(),
            description: "Read-oriented worker for investigation, inspection, and evidence gathering."
                .to_string(),
            role: "researcher".to_string(),
            instructions: "Investigate carefully, gather concrete evidence, and avoid writes unless the task explicitly requires them."
                .to_string(),
            allowed_tools: vec![
                "read_file".to_string(),
                "list_dir".to_string(),
                "file_exists".to_string(),
                "run_command".to_string(),
            ],
            output_contract: Some(
                "Return the relevant findings, the strongest evidence, and any blockers.".to_string(),
            ),
            max_iterations: Some(4),
            max_steps: 6,
            timeout_secs: 90,
            memory_budget: 1200,
        },
        WorkerPreset {
            id: "executor".to_string(),
            name: "Executor".to_string(),
            description: "Change-oriented worker for applying a bounded fix or operation."
                .to_string(),
            role: "executor".to_string(),
            instructions: "Apply the minimum change needed, stay within the assigned scope, and report what changed."
                .to_string(),
            allowed_tools: vec![
                "read_file".to_string(),
                "write_file".to_string(),
                "list_dir".to_string(),
                "file_exists".to_string(),
                "run_command".to_string(),
            ],
            output_contract: Some(
                "Return the change made, the files or commands touched, and any remaining risk."
                    .to_string(),
            ),
            max_iterations: Some(6),
            max_steps: 8,
            timeout_secs: 120,
            memory_budget: 900,
        },
        WorkerPreset {
            id: "verifier".to_string(),
            name: "Verifier".to_string(),
            description: "Validation worker for checks, tests, and final-state confirmation."
                .to_string(),
            role: "verifier".to_string(),
            instructions: "Verify the result, prefer reproducible checks, and call out regressions or gaps clearly."
                .to_string(),
            allowed_tools: vec![
                "read_file".to_string(),
                "list_dir".to_string(),
                "file_exists".to_string(),
                "run_command".to_string(),
            ],
            output_contract: Some(
                "Return verification status, commands run, and any failing evidence.".to_string(),
            ),
            max_iterations: Some(4),
            max_steps: 6,
            timeout_secs: 90,
            memory_budget: 800,
        },
        WorkerPreset {
            id: "summariser".to_string(),
            name: "Summariser".to_string(),
            description: "Synthesis worker for compact final output from prior evidence."
                .to_string(),
            role: "summariser".to_string(),
            instructions: "Synthesize prior results into a concise final answer without redoing the work."
                .to_string(),
            allowed_tools: vec![
                "read_file".to_string(),
                "list_dir".to_string(),
                "file_exists".to_string(),
            ],
            output_contract: Some(
                "Return a concise summary with decisions, outcomes, and follow-up items.".to_string(),
            ),
            max_iterations: Some(3),
            max_steps: 4,
            timeout_secs: 60,
            memory_budget: 600,
        },
    ]
}

pub fn worker_preset(id: &str) -> Result<WorkerPreset> {
    list_worker_presets()
        .into_iter()
        .find(|preset| preset.id == id)
        .ok_or_else(|| anyhow::anyhow!("Unknown worker preset '{}'", id))
}

pub fn execution_profile_for_preset(id: &str) -> Result<TaskExecutionProfile> {
    let preset = worker_preset(id)?;
    Ok(TaskExecutionProfile {
        agent_id: None,
        agent_name: None,
        worker_preset_id: Some(preset.id.clone()),
        worker_preset_name: Some(preset.name.clone()),
        purpose: Some(preset.description.clone()),
        instructions: preset.instructions.clone(),
        allowed_tools: preset.allowed_tools.clone(),
        output_contract: preset.output_contract.clone(),
        max_iterations: preset.max_iterations,
    })
}

pub fn subagent_spec_for_preset(
    id: &str,
    task: impl Into<String>,
    allowed_tools: Vec<String>,
) -> Result<SubagentSpec> {
    let preset = worker_preset(id)?;
    let preset_tools = preset.allowed_tools.clone();
    let tools_allowed = if allowed_tools.is_empty() {
        preset_tools
    } else {
        allowed_tools
            .into_iter()
            .filter(|tool| preset_tools.iter().any(|allowed| allowed == tool))
            .collect()
    };

    Ok(SubagentSpec {
        role: role_from_preset(&preset),
        task: task.into(),
        tools_allowed,
        memory_budget: preset.memory_budget,
        model_override: None,
        max_steps: preset.max_steps,
        timeout_secs: preset.timeout_secs,
    })
}

fn role_from_preset(preset: &WorkerPreset) -> SubagentRole {
    match preset.role.as_str() {
        "researcher" => SubagentRole::Researcher,
        "executor" => SubagentRole::Executor,
        "verifier" => SubagentRole::Verifier,
        "summariser" => SubagentRole::Summariser,
        other => SubagentRole::Custom(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{execution_profile_for_preset, subagent_spec_for_preset};

    #[test]
    fn worker_profile_carries_bounded_iterations() {
        let profile = execution_profile_for_preset("verifier").unwrap();
        assert_eq!(profile.worker_preset_id.as_deref(), Some("verifier"));
        assert_eq!(profile.max_iterations, Some(4));
        assert!(profile
            .allowed_tools
            .iter()
            .any(|tool| tool == "run_command"));
    }

    #[test]
    fn subagent_preset_filters_requested_tools() {
        let spec = subagent_spec_for_preset(
            "summariser",
            "summarise this",
            vec!["read_file".to_string(), "write_file".to_string()],
        )
        .unwrap();
        assert_eq!(spec.tools_allowed, vec!["read_file".to_string()]);
    }
}
