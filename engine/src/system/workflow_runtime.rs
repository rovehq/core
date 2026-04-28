use anyhow::{anyhow, Result};
use sdk::{
    SpecRunStatus, TaskExecutionProfile, WorkflowBranchSpec, WorkflowRunRecord, WorkflowSpec,
    WorkflowStepSpec,
};
use std::collections::BTreeMap;
use uuid::Uuid;

use crate::cli::run::execute_local_task_request;
use crate::config::Config;
use crate::specs::{allowed_tools, SpecRepository};
use crate::storage::{Database, WorkflowStepFinish, WorkflowStepStart};
use crate::system::worker_presets;

pub struct WorkflowExecutionResult {
    pub run: WorkflowRunRecord,
    pub final_output: String,
}

pub async fn start_new_run(
    repo: &SpecRepository,
    db: &Database,
    config: &Config,
    workflow: &WorkflowSpec,
    input: &str,
) -> Result<WorkflowExecutionResult> {
    let run_id = Uuid::new_v4().to_string();
    db.agent_runs()
        .start_workflow_run(&run_id, &workflow.id, input, workflow.steps.len() as i64)
        .await?;
    execute_run(
        repo,
        db,
        config,
        workflow,
        &run_id,
        input.to_string(),
        0,
        input.to_string(),
        BTreeMap::new(),
    )
    .await
}

pub async fn resume_run(
    repo: &SpecRepository,
    db: &Database,
    config: &Config,
    run_id: &str,
) -> Result<WorkflowExecutionResult> {
    let run = db
        .agent_runs()
        .get_workflow_run(run_id)
        .await?
        .ok_or_else(|| anyhow!("Workflow run '{}' was not found", run_id))?;
    if matches!(
        run.status,
        SpecRunStatus::Completed | SpecRunStatus::Canceled
    ) {
        anyhow::bail!("Workflow run '{}' is already settled", run_id);
    }
    if run.cancel_requested {
        anyhow::bail!(
            "Workflow run '{}' already has a pending cancel request",
            run_id
        );
    }

    let workflow = repo.load_workflow(&run.workflow_id)?;
    let steps = db.agent_runs().list_workflow_run_steps(run_id).await?;
    let start_index = run
        .current_step_index
        .or_else(|| {
            let completed = run.steps_completed.max(0) as usize;
            if completed < workflow.steps.len() {
                Some(completed as i64)
            } else {
                None
            }
        })
        .map(|value| value.max(0) as usize)
        .unwrap_or(workflow.steps.len());
    let last_output = steps
        .iter()
        .filter(|step| matches!(step.status, SpecRunStatus::Completed))
        .max_by_key(|step| step.step_index)
        .and_then(|step| step.output.clone())
        .unwrap_or_else(|| run.input.clone());
    let mut variables = BTreeMap::new();
    for step in &steps {
        if matches!(step.status, SpecRunStatus::Completed) {
            if let Some(output) = step.output.as_ref() {
                variables.insert(format!("{}.result", step.step_id), output.clone());
            }
        }
    }

    db.agent_runs().prepare_workflow_resume(run_id).await?;
    execute_run(
        repo,
        db,
        config,
        &workflow,
        run_id,
        run.input.clone(),
        start_index,
        last_output,
        variables,
    )
    .await
}

async fn execute_run(
    repo: &SpecRepository,
    db: &Database,
    config: &Config,
    workflow: &WorkflowSpec,
    run_id: &str,
    input: String,
    start_index: usize,
    mut last_output: String,
    mut variables: BTreeMap<String, String>,
) -> Result<WorkflowExecutionResult> {
    let mut next_index = start_index;
    while next_index < workflow.steps.len() {
        if db
            .agent_runs()
            .workflow_run_cancel_requested(run_id)
            .await?
        {
            return finish_canceled_run(db, run_id, &last_output).await;
        }

        let index = next_index;
        let step = &workflow.steps[index];
        let rendered = render_step_prompt(&step.prompt, &input, &last_output, &variables);
        let profile = execution_profile_for_step(repo, step)?;
        db.agent_runs()
            .record_workflow_step_start(WorkflowStepStart {
                run_id,
                step_index: index as i64,
                step_id: &step.id,
                step_name: &step.name,
                agent_id: step.agent_id.as_deref(),
                worker_preset: step.worker_preset.as_deref(),
                prompt: &rendered,
            })
            .await?;

        match execute_local_task_request(
            rendered,
            config,
            sdk::RunMode::Serial,
            sdk::RunIsolation::None,
            profile,
        )
        .await
        {
            Ok(task_result) => {
                last_output = task_result.answer.clone();
                variables.insert(format!("{}.result", step.id), task_result.answer.clone());
                next_index = resolve_next_step_index(workflow, index, &task_result.answer);
                db.agent_runs()
                    .record_workflow_step_finish(WorkflowStepFinish {
                        run_id,
                        step_index: index as i64,
                        status: SpecRunStatus::Completed,
                        task_id: Some(&task_result.task_id),
                        output: Some(&task_result.answer),
                        error: None,
                    })
                    .await?;
                if db
                    .agent_runs()
                    .workflow_run_cancel_requested(run_id)
                    .await?
                {
                    return finish_canceled_run(db, run_id, &last_output).await;
                }
            }
            Err(error) => {
                db.agent_runs()
                    .record_workflow_step_finish(WorkflowStepFinish {
                        run_id,
                        step_index: index as i64,
                        status: SpecRunStatus::Failed,
                        task_id: None,
                        output: None,
                        error: Some(&error.to_string()),
                    })
                    .await?;
                if db
                    .agent_runs()
                    .workflow_run_cancel_requested(run_id)
                    .await?
                {
                    return finish_canceled_run(db, run_id, &last_output).await;
                }
                db.agent_runs()
                    .finish_workflow_run(
                        run_id,
                        SpecRunStatus::Failed,
                        None,
                        Some(&error.to_string()),
                    )
                    .await?;
                return Err(error);
            }
        }
    }

    db.agent_runs()
        .finish_workflow_run(run_id, SpecRunStatus::Completed, Some(&last_output), None)
        .await?;

    let run = db
        .agent_runs()
        .get_workflow_run(run_id)
        .await?
        .ok_or_else(|| anyhow!("Workflow run '{}' disappeared after execution", run_id))?;

    Ok(WorkflowExecutionResult {
        run,
        final_output: last_output,
    })
}

async fn finish_canceled_run(
    db: &Database,
    run_id: &str,
    last_output: &str,
) -> Result<WorkflowExecutionResult> {
    db.agent_runs()
        .finish_workflow_run(run_id, SpecRunStatus::Canceled, Some(last_output), None)
        .await?;

    let run = db
        .agent_runs()
        .get_workflow_run(run_id)
        .await?
        .ok_or_else(|| anyhow!("Workflow run '{}' disappeared after cancellation", run_id))?;

    Ok(WorkflowExecutionResult {
        run,
        final_output: last_output.to_string(),
    })
}

fn execution_profile_for_step(
    repo: &SpecRepository,
    step: &WorkflowStepSpec,
) -> Result<Option<TaskExecutionProfile>> {
    if let Some(agent_id) = step.agent_id.as_deref() {
        let spec = repo.load_agent(agent_id)?;
        return Ok(Some(TaskExecutionProfile {
            agent_id: Some(spec.id.clone()),
            agent_name: Some(spec.name.clone()),
            thread_id: None,
            worker_preset_id: None,
            worker_preset_name: None,
            purpose: Some(spec.purpose.clone()),
            instructions: spec.instructions.clone(),
            allowed_tools: allowed_tools(&spec),
            callable_agents: spec.callable_agents.clone(),
            output_contract: spec.output_contract.clone(),
            outcome_contract: step
                .outcome_contract
                .clone()
                .or_else(|| spec.outcome_contract.clone()),
            max_iterations: None,
        }));
    }
    if let Some(worker_preset) = step.worker_preset.as_deref() {
        return Ok(Some(worker_presets::execution_profile_for_preset(
            worker_preset,
        )?));
    }
    Ok(None)
}

fn render_step_prompt(
    template: &str,
    input: &str,
    last_output: &str,
    variables: &BTreeMap<String, String>,
) -> String {
    let mut rendered = template
        .replace("{{input}}", input)
        .replace("{{last_output}}", last_output);

    for (name, value) in variables {
        rendered = rendered.replace(&format!("{{{{{name}}}}}"), value);
    }

    // If the template didn't reference {{last_output}} explicitly but a prior
    // step produced output, prepend it so the agent has context to work with.
    if !last_output.is_empty()
        && !template.contains("{{last_output}}")
        && last_output != input
    {
        rendered = format!("Previous step output:\n{last_output}\n\n{rendered}");
    }

    rendered
}

fn resolve_next_step_index(workflow: &WorkflowSpec, current_index: usize, output: &str) -> usize {
    let Some(step) = workflow.steps.get(current_index) else {
        return workflow.steps.len();
    };

    if let Some(target_id) = resolve_branch_target(step, output) {
        if let Some(index) = workflow
            .steps
            .iter()
            .position(|candidate| candidate.id == target_id)
        {
            return index;
        }
    }

    current_index + 1
}

fn resolve_branch_target<'a>(step: &'a WorkflowStepSpec, output: &str) -> Option<&'a str> {
    let normalized_output = output.to_ascii_lowercase();
    step.branches
        .iter()
        .find(|branch| branch_matches(branch, &normalized_output))
        .map(|branch| branch.next_step_id.as_str())
}

fn branch_matches(branch: &WorkflowBranchSpec, normalized_output: &str) -> bool {
    normalized_output.contains(&branch.contains.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::{branch_matches, render_step_prompt, resolve_next_step_index};
    use sdk::{WorkflowBranchSpec, WorkflowSpec, WorkflowStepSpec};
    use std::collections::BTreeMap;

    #[test]
    fn render_step_prompt_replaces_named_step_result_variables() {
        let mut variables = BTreeMap::new();
        variables.insert("inspect.result".to_string(), "found issue".to_string());
        variables.insert("fix.result".to_string(), "patched file".to_string());

        let rendered = render_step_prompt(
            "Input={{input}}\nLast={{last_output}}\nInspect={{inspect.result}}\nFix={{fix.result}}",
            "ship it",
            "checked status",
            &variables,
        );

        assert!(rendered.contains("Input=ship it"));
        assert!(rendered.contains("Last=checked status"));
        assert!(rendered.contains("Inspect=found issue"));
        assert!(rendered.contains("Fix=patched file"));
    }

    #[test]
    fn branch_match_is_case_insensitive_contains() {
        let branch = WorkflowBranchSpec {
            contains: "retry".to_string(),
            next_step_id: "retry-step".to_string(),
        };

        assert!(branch_matches(&branch, "needs retry before ship"));
        assert!(branch_matches(
            &branch,
            "needs RETRY before ship".to_ascii_lowercase().as_str()
        ));
    }

    #[test]
    fn resolve_next_step_index_uses_branch_target_before_linear_progression() {
        let workflow = WorkflowSpec {
            id: "branchy".to_string(),
            name: "Branchy".to_string(),
            steps: vec![
                WorkflowStepSpec {
                    id: "inspect".to_string(),
                    name: "Inspect".to_string(),
                    prompt: "inspect".to_string(),
                    agent_id: None,
                    worker_preset: None,
                    thread_key: None,
                    outcome_contract: None,
                    continue_on_error: false,
                    branches: vec![WorkflowBranchSpec {
                        contains: "retry".to_string(),
                        next_step_id: "fix".to_string(),
                    }],
                },
                WorkflowStepSpec {
                    id: "verify".to_string(),
                    name: "Verify".to_string(),
                    prompt: "verify".to_string(),
                    agent_id: None,
                    worker_preset: None,
                    thread_key: None,
                    outcome_contract: None,
                    continue_on_error: false,
                    branches: Vec::new(),
                },
                WorkflowStepSpec {
                    id: "fix".to_string(),
                    name: "Fix".to_string(),
                    prompt: "fix".to_string(),
                    agent_id: None,
                    worker_preset: None,
                    thread_key: None,
                    outcome_contract: None,
                    continue_on_error: false,
                    branches: Vec::new(),
                },
            ],
            ..WorkflowSpec::default()
        };

        assert_eq!(
            resolve_next_step_index(&workflow, 0, "Please RETRY with a patch"),
            2
        );
        assert_eq!(resolve_next_step_index(&workflow, 1, "all good"), 2);
    }
}
