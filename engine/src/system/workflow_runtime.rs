use anyhow::{anyhow, Result};
use sdk::{SpecRunStatus, TaskExecutionProfile, WorkflowRunRecord, WorkflowSpec, WorkflowStepSpec};
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
    if matches!(run.status, SpecRunStatus::Completed) {
        anyhow::bail!("Workflow run '{}' is already completed", run_id);
    }

    let workflow = repo.load_workflow(&run.workflow_id)?;
    let steps = db.agent_runs().list_workflow_run_steps(run_id).await?;
    let start_index = run.steps_completed.max(0) as usize;
    let last_output = steps
        .iter()
        .filter(|step| matches!(step.status, SpecRunStatus::Completed))
        .max_by_key(|step| step.step_index)
        .and_then(|step| step.output.clone())
        .unwrap_or_else(|| run.input.clone());

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
) -> Result<WorkflowExecutionResult> {
    for (index, step) in workflow.steps.iter().enumerate().skip(start_index) {
        let rendered = render_step_prompt(&step.prompt, &input, &last_output);
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

fn execution_profile_for_step(
    repo: &SpecRepository,
    step: &WorkflowStepSpec,
) -> Result<Option<TaskExecutionProfile>> {
    if let Some(agent_id) = step.agent_id.as_deref() {
        let spec = repo.load_agent(agent_id)?;
        return Ok(Some(TaskExecutionProfile {
            agent_id: Some(spec.id.clone()),
            agent_name: Some(spec.name.clone()),
            worker_preset_id: None,
            worker_preset_name: None,
            purpose: Some(spec.purpose.clone()),
            instructions: spec.instructions.clone(),
            allowed_tools: allowed_tools(&spec),
            output_contract: spec.output_contract.clone(),
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

fn render_step_prompt(template: &str, input: &str, last_output: &str) -> String {
    template
        .replace("{{input}}", input)
        .replace("{{last_output}}", last_output)
}
